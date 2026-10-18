//! Manual update check from the tray menu.
//!
//! Network I/O runs on a worker thread so the message loop never blocks;
//! the actual file swap is done by `kcfix-updater.exe` after this process
//! exits.

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::*,
};

use super::{dialog::msg_box, ids::WM_UPDATE_EXIT, state::current_lang, window::update_exit_nonce};
use crate::i18n::{fill, strings, Language};

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
        let manifest = match classify_update_result(crate::updater::check_for_update(), lang) {
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
        // records stay right.
        if let Some((manager, command)) = std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(package_manager)
        {
            crate::updater::end_update();
            let text = fill(s.update_managed_fmt, &[&manifest.version, manager, command]);
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
