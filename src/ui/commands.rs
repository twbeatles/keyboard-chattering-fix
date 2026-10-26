//! User commands shared by the menu bar, the tray menu, and buttons.
//!
//! Every entry point resolves to a `MENU_*` id and lands in [`run`], so a
//! feature reachable from three places has exactly one implementation.

use windows::{
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

use super::{
    dialog::{msg_box, show_about},
    elevation, files, foreground, history,
    ids::*,
    settings_form::persist_live,
    state::current_lang,
    stats::update_stats,
    text::to_wide,
    tray,
    update::spawn_update_check,
};
use crate::{app::state::SHARED, config, i18n::strings};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// Bring the window back from the tray (or from minimized).
pub(crate) unsafe fn show_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

pub(crate) unsafe fn hide_to_tray(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        tray::notify_hidden_once(hwnd);
    }
}

/// Turn protection on/off immediately; the tick syncs pill, toggle, tray.
/// Either choice ends a pause for a set time: the user decided.
pub(crate) unsafe fn set_protection(hwnd: HWND, on: bool) {
    unsafe {
        if let Some(g) = SHARED.get() {
            g.lock().unwrap().enabled = on;
        }
        if foreground::timed_pause_active() {
            foreground::set_timed_pause(None);
            foreground::tick(hwnd);
            super::stats::invalidate();
        }
        update_stats(hwnd);
    }
}

/// Local `HH:MM` that is `minutes` after `now_minute_of_day` (wraps at
/// midnight): when a pause for a set time ends.
pub(crate) fn clock_after(now_minute_of_day: u32, minutes: u32) -> String {
    let t = (now_minute_of_day + minutes) % (24 * 60);
    format!("{:02}:{:02}", t / 60, t % 60)
}

/// Pause protection for `minutes` (tray and Tools menus). Protection is on
/// again afterwards, even if it was off before: that is what "pause" means.
/// Not saved: a restart during the pause starts protected.
pub(crate) unsafe fn pause_for(hwnd: HWND, minutes: u32) {
    unsafe {
        let now = windows::Win32::System::SystemInformation::GetLocalTime();
        let ends = clock_after(u32::from(now.wHour) * 60 + u32::from(now.wMinute), minutes);
        let until = Instant::now() + Duration::from_secs(u64::from(minutes) * 60);
        if let Some(g) = SHARED.get() {
            g.lock().unwrap().enabled = true;
        }
        foreground::set_timed_pause(Some((until, ends)));
        // Release delay: the hook stops judging events while paused, so a
        // held-back release must not be sent after a press that passed.
        crate::input::hook::flush_deferred_releases();
        foreground::tick(hwnd);
        super::stats::invalidate();
        update_stats(hwnd);
    }
}

/// Timer tick: end a pause for a set time once it has run out.
pub(crate) unsafe fn pause_tick(hwnd: HWND) {
    unsafe {
        if foreground::end_expired_pause(Instant::now()) {
            foreground::tick(hwnd);
            super::stats::invalidate();
            update_stats(hwnd);
            tray::balloon(hwnd, strings(current_lang()).pause_ended);
        }
    }
}

fn protection_on() -> bool {
    SHARED.get().is_none_or(|g| g.lock().unwrap().enabled)
}

/// When cleaning mode was turned on from the UI (for its time limit).
static CLEANING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) unsafe fn set_cleaning(hwnd: HWND, on: bool) {
    unsafe {
        if let Some(g) = SHARED.get() {
            g.lock().unwrap().debouncer.cleaning_mode = on;
        }
        if on {
            // Held-back releases go out now, before input freezes: cleaning must
            // not start with keys that look stuck down (ISSUE-002).
            crate::input::hook::flush_deferred_releases();
        }
        *CLEANING_SINCE.lock().unwrap() = on.then(Instant::now);
        if on {
            show_window(hwnd);
        }
        update_stats(hwnd);
    }
}

/// Whether cleaning mode has run out of time (`limit_min` 0 = never).
pub(crate) fn cleaning_expired(elapsed: Duration, limit_min: u64) -> bool {
    limit_min > 0 && elapsed >= Duration::from_secs(limit_min * 60)
}

/// Timer tick: end cleaning mode after its time limit, so a forgotten
/// cleaning session never leaves the keyboard dead.
pub(crate) unsafe fn cleaning_tick(hwnd: HWND) {
    unsafe {
        let since = *CLEANING_SINCE.lock().unwrap();
        let Some(since) = since else {
            return;
        };
        let active = SHARED
            .get()
            .is_some_and(|g| g.lock().unwrap().debouncer.cleaning_mode);
        if !active {
            // Ended with Enter or the button.
            *CLEANING_SINCE.lock().unwrap() = None;
            return;
        }
        let limit = super::state::SAVED
            .get()
            .map_or(5, |s| s.lock().unwrap().cleaning_timeout_min);
        if cleaning_expired(since.elapsed(), limit) {
            set_cleaning(hwnd, false);
            tray::balloon(hwnd, strings(current_lang()).cleaning_auto_ended);
        }
    }
}

/// Open a folder or URL with the shell on a worker thread: `ShellExecuteW`
/// can take seconds (cold browser or Explorer start), which must not freeze
/// the window (G7). The shell requires COM on the calling thread.
fn shell_open(target: String) {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    std::thread::spawn(move || unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let t = to_wide(&target);
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(t.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if com.is_ok() {
            CoUninitialize();
        }
    });
}

pub(crate) unsafe fn run(hwnd: HWND, cmd: usize) {
    unsafe {
        let s = strings(current_lang());
        match cmd {
            MENU_SHOW => show_window(hwnd),
            MENU_HIDE => hide_to_tray(hwnd),
            // While paused for a set time, the toggle (pill, hotkey, menu)
            // resumes instead of switching protection off.
            MENU_TOGGLE => {
                set_protection(hwnd, foreground::timed_pause_active() || !protection_on())
            }
            MENU_PAUSE_15 => pause_for(hwnd, 15),
            MENU_PAUSE_60 => pause_for(hwnd, 60),
            MENU_CLEANING => set_cleaning(hwnd, true),
            MENU_RESET_STATS => {
                let answer = msg_box(
                    hwnd,
                    s.reset_stats_btn,
                    s.reset_confirm,
                    MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                );
                if answer == IDYES {
                    if let Some(g) = SHARED.get() {
                        g.lock().unwrap().debouncer.reset_stats();
                    }
                    persist_live();
                    update_stats(hwnd);
                    history::refresh(hwnd, true);
                }
            }
            MENU_CLEAR_LOG => {
                if let Some(g) = SHARED.get() {
                    g.lock().unwrap().debouncer.clear_log();
                }
                history::refresh(hwnd, true);
            }
            MENU_OPEN_CPL => {
                let _ = std::process::Command::new("control.exe")
                    .arg("keyboard")
                    .spawn();
            }
            MENU_OPEN_CONFIG => {
                if let Some(dir) = config::config_path().parent() {
                    let _ = std::fs::create_dir_all(dir);
                    shell_open(dir.to_string_lossy().into_owned());
                }
            }
            MENU_UPDATE => spawn_update_check(hwnd),
            MENU_HOMEPAGE => shell_open(config::HOMEPAGE_URL.to_string()),
            MENU_ABOUT => show_about(hwnd),
            MENU_EXPORT_HISTORY => files::export_history(hwnd),
            MENU_EXPORT_SETTINGS => files::export_settings(hwnd),
            MENU_IMPORT_SETTINGS => files::import_settings(hwnd),
            MENU_RUN_ELEVATED => elevation::relaunch_elevated(hwnd),
            MENU_ELEVATED_AUTOSTART => elevation::toggle_task(hwnd),
            MENU_EXIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_end_is_shown_as_a_local_clock_time() {
        assert_eq!(clock_after(14 * 60 + 5, 15), "14:20");
        assert_eq!(clock_after(23 * 60 + 50, 60), "00:50");
        assert_eq!(clock_after(0, 0), "00:00");
    }

    #[test]
    fn cleaning_limit_counts_minutes_and_zero_disables_it() {
        assert!(!cleaning_expired(Duration::from_secs(299), 5));
        assert!(cleaning_expired(Duration::from_secs(300), 5));
        assert!(!cleaning_expired(Duration::from_secs(86_400), 0));
    }
}
