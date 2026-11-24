//! Manual update check from the tray menu, plus the automatic weekly check
//! (tray balloon only, never a dialog).
//!
//! Network I/O runs on a worker thread so the message loop never blocks;
//! the actual file swap is done by `kcfix-updater.exe` after this process
//! exits.

use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::*,
};

use super::{
    dialog::msg_box, ids::WM_UPDATE_EXIT, state::current_lang, tray, window::update_exit_nonce,
};
use crate::i18n::{Language, fill, strings};

/// Automatic checks run at most this often (manual checks anytime).
pub(crate) const AUTO_UPDATE_INTERVAL_SECS: u64 = 7 * 24 * 3600;
/// After a failed check the next one waits only this long.
pub(crate) const UPDATE_FAIL_RETRY_SECS: u64 = 24 * 3600;
/// The automatic check waits this long after startup (network settle).
pub(crate) const AUTO_UPDATE_STARTUP_DELAY_SECS: u64 = 60;
/// How long the automatic check backs off after its result could not be
/// recorded (settings file unwritable): without this the timer would start
/// a fresh check on every tick, since the schedule never advances
/// (ISSUE-003).
pub(crate) const AUTO_PERSIST_FAIL_BACKOFF_SECS: u64 = 3600;

/// Whether an automatic check is due: enabled, past the startup delay, and
/// a week since the last finished check (manual or automatic).
pub(crate) fn auto_check_due(
    enabled: bool,
    last_unix: u64,
    now_unix: u64,
    uptime_secs: u64,
) -> bool {
    enabled
        && uptime_secs >= AUTO_UPDATE_STARTUP_DELAY_SECS
        && now_unix.saturating_sub(last_unix) >= AUTO_UPDATE_INTERVAL_SECS
}

/// Timestamp a failed check records: the next automatic check runs a day
/// after the failure instead of a week after it.
pub(crate) fn fail_retry_stamp(now_unix: u64) -> u64 {
    now_unix.saturating_sub(AUTO_UPDATE_INTERVAL_SECS - UPDATE_FAIL_RETRY_SECS)
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A balloon click opens the manual update flow only while the automatic
/// announcement is still fresh: Windows dismisses balloons after seconds,
/// so an hours-old flag must not reroute an unrelated balloon click
/// (ISSUE-002).
pub(crate) const BALLOON_CLICK_WINDOW_SECS: u64 = 300;

/// When the automatic check announced an update (Unix time, 0 = none).
static UPDATE_BALLOON_AT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn take_update_balloon() -> bool {
    balloon_click_accepts(UPDATE_BALLOON_AT.swap(0, Ordering::Relaxed), unix_now())
}

fn balloon_click_accepts(shown_at: u64, now: u64) -> bool {
    shown_at != 0 && now.saturating_sub(shown_at) < BALLOON_CLICK_WINDOW_SECS
}

/// The next automatic check runs no earlier than this (Unix time, 0 =
/// none): a failed record backs the timer off in memory.
static AUTO_BACKOFF_UNTIL: AtomicU64 = AtomicU64::new(0);

fn auto_spawn_allowed(backoff_until: u64, now: u64) -> bool {
    backoff_until <= now
}

fn note_check_result(checked: &UpdatePrompt, now_unix: u64) {
    let stamp = match checked {
        UpdatePrompt::Failed(_) => fail_retry_stamp(now_unix),
        _ => now_unix,
    };
    if super::settings_form::note_update_checked(stamp) {
        AUTO_BACKOFF_UNTIL.store(0, Ordering::Relaxed);
    } else {
        AUTO_BACKOFF_UNTIL.store(
            now_unix.saturating_add(AUTO_PERSIST_FAIL_BACKOFF_SECS),
            Ordering::Relaxed,
        );
    }
}

/// Pure classification of an update-check result for the tray flow, split
/// out so the branches are unit-testable without windows or network.
#[derive(Debug)]
enum UpdatePrompt {
    UpToDate,
    Available(crate::updater::UpdateManifest),
    /// Failure text already rendered in the UI language (ISSUE-006).
    Failed(String),
}

fn classify_update_result(
    result: Result<crate::updater::UpdateManifest, crate::updater::UpdateError>,
    lang: Language,
) -> UpdatePrompt {
    match result {
        Err(crate::updater::UpdateError::NoUpdate) => UpdatePrompt::UpToDate,
        Ok(manifest) => UpdatePrompt::Available(manifest),
        Err(err) => UpdatePrompt::Failed(err.message(lang)),
    }
}

/// The package manager that installed `exe`, with its update command.
fn package_manager(exe: &std::path::Path) -> Option<(&'static str, &'static str)> {
    let path = exe.to_string_lossy().to_lowercase().replace('/', "\\");
    if path.contains("\\scoop\\apps\\") {
        Some(("Scoop", "scoop update keyboard-chattering-fix"))
    } else if path.contains("\\winget\\packages\\") || path.contains("\\winget\\links\\") {
        Some(("winget", "winget upgrade twbeatles.KeyboardChatteringFix"))
    } else {
        None
    }
}

/// How an available update gets installed.
#[derive(Debug, PartialEq, Eq)]
enum UpdateRoute {
    /// Installed by a package manager: its own update command.
    Managed(&'static str, &'static str),
    /// Running as administrator: the in-app update would run the helper
    /// and install a file staged in the user's %TEMP% (writable by any
    /// program of this account) with administrator rights, so ask for a
    /// normal start instead (2026-10-22 audit ISSUE-003).
    Elevated,
    InApp,
}

fn update_route(exe: Option<&std::path::Path>, elevated: bool) -> UpdateRoute {
    match exe.and_then(package_manager) {
        Some((manager, command)) => UpdateRoute::Managed(manager, command),
        None if elevated => UpdateRoute::Elevated,
        None => UpdateRoute::InApp,
    }
}

pub(crate) fn spawn_update_check(hwnd: HWND) {
    let raw_hwnd = hwnd.0 as usize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw_hwnd as *mut _);
        let lang = current_lang();
        let s = strings(lang);
        let fail = |text: &str| unsafe {
            msg_box(hwnd, s.update_failed_title, text, MB_OK | MB_ICONERROR);
        };
        if !crate::updater::try_begin_update() {
            unsafe {
                msg_box(
                    hwnd,
                    s.update_title,
                    s.update_busy,
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            return;
        }
        let checked = classify_update_result(crate::updater::check_for_update(), lang);
        // A manual check satisfies the automatic schedule too.
        note_check_result(&checked, unix_now());
        let manifest = match checked {
            UpdatePrompt::UpToDate => {
                crate::updater::end_update();
                let text = s.update_latest.replacen("{}", crate::config::VERSION, 1);
                unsafe {
                    msg_box(hwnd, s.update_title, &text, MB_OK | MB_ICONINFORMATION);
                }
                return;
            }
            UpdatePrompt::Failed(message) => {
                crate::updater::end_update();
                fail(&message);
                return;
            }
            UpdatePrompt::Available(manifest) => manifest,
        };
        // Installed by a package manager: let it do the update, so its
        // records stay right. Running elevated: no in-app update at all.
        let exe = std::env::current_exe().ok();
        let notice = match update_route(exe.as_deref(), super::foreground::self_elevated()) {
            UpdateRoute::Managed(manager, command) => Some(fill(
                s.update_managed_fmt,
                &[&manifest.version, manager, command],
            )),
            UpdateRoute::Elevated => Some(fill(s.update_elevated_fmt, &[&manifest.version])),
            UpdateRoute::InApp => None,
        };
        if let Some(text) = notice {
            crate::updater::end_update();
            unsafe {
                msg_box(hwnd, s.update_title, &text, MB_OK | MB_ICONINFORMATION);
            }
            return;
        }
        // The only confirmation: it also says the program restarts, so
        // nothing modal sits between launching the helper and exiting.
        let text = fill(
            s.update_available,
            &[&manifest.version, crate::config::VERSION],
        );
        let answer = unsafe { msg_box(hwnd, s.update_title, &text, MB_YESNO | MB_ICONINFORMATION) };
        if answer != IDYES {
            crate::updater::end_update();
            return;
        }
        let staged = match crate::updater::prepare_update(&manifest) {
            Ok(staged) => staged,
            Err(err) => {
                crate::updater::end_update();
                fail(&err.message(lang));
                return;
            }
        };
        if let Err(err) = crate::updater::launch_helper(&staged, lang) {
            crate::updater::discard_staged(&staged);
            crate::updater::end_update();
            fail(&err.message(lang));
            return;
        }
        // The helper is now waiting for this process to exit: exit right
        // away. A modal notice here used to eat into the helper's wait and
        // could strand the update with no filter running (ISSUE-003).
        // (Single-flight flag intentionally left set.)
        unsafe {
            let _ = PostMessageW(
                Some(hwnd),
                WM_UPDATE_EXIT,
                WPARAM(update_exit_nonce() as usize),
                LPARAM(0),
            );
        }
    });
}

/// Silent automatic check: runs on the 500 ms timer when due, announces an
/// available update with a tray balloon only (no dialog), and stays quiet
/// otherwise. A day after a failure it retries (see [`fail_retry_stamp`]).
pub(crate) fn spawn_auto_check(hwnd: HWND) {
    let raw_hwnd = hwnd.0 as usize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw_hwnd as *mut _);
        if !crate::updater::try_begin_update() {
            return;
        }
        let lang = current_lang();
        let s = strings(lang);
        let checked = classify_update_result(crate::updater::check_for_update(), lang);
        note_check_result(&checked, unix_now());
        match checked {
            UpdatePrompt::Available(manifest) => {
                crate::updater::end_update();
                let text = fill(s.update_available_balloon_fmt, &[&manifest.version]);
                UPDATE_BALLOON_AT.store(unix_now(), Ordering::Relaxed);
                unsafe {
                    tray::balloon(hwnd, &text);
                }
            }
            // Up to date or failed: nothing to show. Failures already
            // rescheduled themselves with a sooner stamp.
            _ => crate::updater::end_update(),
        }
    });
}

/// 500 ms timer hook: start an automatic check when one is due. Cheap
/// (two loads and the clock); the network runs on a worker thread.
pub(crate) fn auto_tick(hwnd: HWND) {
    if crate::updater::update_in_progress() {
        return;
    }
    let Some(saved) = super::state::SAVED.get() else {
        return;
    };
    let (enabled, last) = {
        let saved = saved.lock().unwrap();
        (saved.auto_check_update, saved.last_update_check_unix)
    };
    let uptime = crate::app::state::START
        .get()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let now = unix_now();
    if !auto_spawn_allowed(AUTO_BACKOFF_UNTIL.load(Ordering::Relaxed), now) {
        return;
    }
    if auto_check_due(enabled, last, now, uptime) {
        spawn_auto_check(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> crate::updater::UpdateManifest {
        crate::updater::UpdateManifest {
            version: "0.2.0".to_string(),
            tag: "v0.2.0".to_string(),
            artifact_url: "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe".to_string(),
            sha256: "0".repeat(64),
            size: 1024,
        }
    }

    #[test]
    fn package_manager_installs_are_recognized() {
        use std::path::Path;
        let scoop = Path::new(
            r"C:\Users\kim\scoop\apps\keyboard-chattering-fix\current\KeyboardChatteringFix.exe",
        );
        assert_eq!(package_manager(scoop).unwrap().0, "Scoop");
        let winget = Path::new(
            r"C:\Users\kim\AppData\Local\Microsoft\WinGet\Packages\twbeatles.KeyboardChatteringFix_x\KeyboardChatteringFix.exe",
        );
        assert_eq!(package_manager(winget).unwrap().0, "winget");
        assert_eq!(
            package_manager(Path::new(r"D:\Tools\KeyboardChatteringFix.exe")),
            None
        );
    }

    #[test]
    fn elevated_copies_never_update_in_app() {
        use std::path::Path;
        let portable = Path::new(r"D:\Tools\KeyboardChatteringFix.exe");
        assert_eq!(update_route(Some(portable), false), UpdateRoute::InApp);
        assert_eq!(update_route(Some(portable), true), UpdateRoute::Elevated);
        assert_eq!(update_route(None, true), UpdateRoute::Elevated);
        // A package manager's own command is safe either way.
        let scoop = Path::new(r"C:\Users\kim\scoop\apps\keyboard-chattering-fix\current\k.exe");
        assert!(matches!(
            update_route(Some(scoop), true),
            UpdateRoute::Managed("Scoop", _)
        ));
    }

    #[test]
    fn classify_maps_no_update_to_up_to_date() {
        assert!(matches!(
            classify_update_result(Err(crate::updater::UpdateError::NoUpdate), Language::En),
            UpdatePrompt::UpToDate
        ));
    }

    #[test]
    fn classify_carries_manifest_for_available() {
        match classify_update_result(Ok(sample_manifest()), Language::En) {
            UpdatePrompt::Available(m) => assert_eq!(m.version, "0.2.0"),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn auto_check_runs_weekly_after_a_minute_uptime() {
        let week = AUTO_UPDATE_INTERVAL_SECS;
        let now = 1_800_000_000u64;
        // Disabled, too early after startup, or checked recently: quiet.
        assert!(!auto_check_due(false, 0, now, 3600));
        assert!(!auto_check_due(true, 0, now, 59));
        assert!(!auto_check_due(true, now - week + 1, now, 3600));
        // Never checked, or a week since the last check: due.
        assert!(auto_check_due(true, 0, now, 60));
        assert!(auto_check_due(true, now - week, now, 3600));
        // The clock went backwards: wait for time to catch up, don't fire.
        assert!(!auto_check_due(true, now + 100, now, 3600));
    }

    #[test]
    fn balloon_click_only_counts_while_the_announcement_is_fresh() {
        assert!(balloon_click_accepts(1000, 1100));
        assert!(!balloon_click_accepts(1000, 1300));
        assert!(!balloon_click_accepts(0, 1100));
        // A skewed clock accepts the click instead of misrouting silently:
        // the worst case is one update dialog.
        assert!(balloon_click_accepts(2000, 1000));
    }

    #[test]
    fn persist_failure_backs_off_the_next_automatic_check() {
        assert!(auto_spawn_allowed(0, 1000));
        assert!(auto_spawn_allowed(1000, 1000));
        assert!(!auto_spawn_allowed(2000, 1000));
    }

    #[test]
    fn failure_retries_a_day_later_not_a_week() {
        let now = 1_800_000_000u64;
        assert_eq!(
            fail_retry_stamp(now),
            now - (AUTO_UPDATE_INTERVAL_SECS - UPDATE_FAIL_RETRY_SECS)
        );
        // A zero clock never underflows.
        assert_eq!(fail_retry_stamp(0), 0);
    }

    #[test]
    fn classify_renders_failure_in_ui_language() {
        use crate::updater::{UpdateError, UpdateErrorKind};
        let err = || UpdateError::with_detail(UpdateErrorKind::Download, "boom");
        match classify_update_result(Err(err()), Language::En) {
            UpdatePrompt::Failed(message) => {
                assert_eq!(message, "Could not download the update.\n(boom)")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        match classify_update_result(Err(err()), Language::Ko) {
            UpdatePrompt::Failed(message) => {
                assert!(message.starts_with("업데이트 파일을"), "{message}");
                assert!(message.ends_with("(boom)"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
