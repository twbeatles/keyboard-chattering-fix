//! Manual update check from the tray menu.
//!
//! Network I/O runs on a worker thread so the message loop never blocks;
//! the actual file swap is done by `kcfix-updater.exe` after this process
//! exits.

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::*,
};

use super::{dialog::msg_box, ids::WM_UPDATE_EXIT, state::current_lang};
use crate::i18n::strings;

/// Pure classification of an update-check result for the tray flow, split
/// out so the branches are unit-testable without windows or network.
#[derive(Debug)]
enum UpdatePrompt {
    UpToDate,
    Available(crate::updater::UpdateManifest),
    Failed(String),
}

fn classify_update_result(
    result: Result<crate::updater::UpdateManifest, crate::updater::UpdateError>,
) -> UpdatePrompt {
    match result {
        Err(crate::updater::UpdateError::NoUpdate) => UpdatePrompt::UpToDate,
        Ok(manifest) => UpdatePrompt::Available(manifest),
        Err(err) => UpdatePrompt::Failed(err.to_string()),
    }
}

pub(crate) fn spawn_update_check(hwnd: HWND) {
    let raw_hwnd = hwnd.0 as usize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw_hwnd as *mut _);
        let s = strings(current_lang());
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
        match classify_update_result(crate::updater::check_for_update()) {
            UpdatePrompt::UpToDate => {
                crate::updater::end_update();
                let text = s.update_latest.replacen("{}", crate::config::VERSION, 1);
                unsafe {
                    msg_box(hwnd, s.update_title, &text, MB_OK | MB_ICONINFORMATION);
                }
            }
            UpdatePrompt::Available(manifest) => {
                let text = s
                    .update_available
                    .replacen("{}", &manifest.version, 1)
                    .replacen("{}", crate::config::VERSION, 1);
                let answer =
                    unsafe { msg_box(hwnd, s.update_title, &text, MB_YESNO | MB_ICONINFORMATION) };
                if answer != IDYES {
                    crate::updater::end_update();
                    return;
                }
                match crate::updater::prepare_update(&manifest) {
                    Ok(staged) => {
                        if let Err(err) = crate::updater::launch_helper(&staged) {
                            crate::updater::discard_staged(&staged);
                            crate::updater::end_update();
                            unsafe {
                                msg_box(
                                    hwnd,
                                    s.update_failed_title,
                                    &err.to_string(),
                                    MB_OK | MB_ICONERROR,
                                );
                            }
                            return;
                        }
                        // The helper owns the swap from here; this process
                        // must exit so the executable file can be replaced.
                        // (Single-flight flag intentionally left set.)
                        unsafe {
                            msg_box(
                                hwnd,
                                s.update_title,
                                s.update_applying,
                                MB_OK | MB_ICONINFORMATION,
                            );
                            let _ = PostMessageW(Some(hwnd), WM_UPDATE_EXIT, WPARAM(0), LPARAM(0));
                        }
                    }
                    Err(err) => {
                        crate::updater::end_update();
                        unsafe {
                            msg_box(
                                hwnd,
                                s.update_failed_title,
                                &err.to_string(),
                                MB_OK | MB_ICONERROR,
                            );
                        }
                    }
                }
            }
            UpdatePrompt::Failed(message) => {
                crate::updater::end_update();
                unsafe {
                    msg_box(hwnd, s.update_failed_title, &message, MB_OK | MB_ICONERROR);
                }
            }
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
    fn classify_maps_no_update_to_up_to_date() {
        assert!(matches!(
            classify_update_result(Err(crate::updater::UpdateError::NoUpdate)),
            UpdatePrompt::UpToDate
        ));
    }

    #[test]
    fn classify_carries_manifest_for_available() {
        match classify_update_result(Ok(sample_manifest())) {
            UpdatePrompt::Available(m) => assert_eq!(m.version, "0.2.0"),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn classify_preserves_error_message() {
        match classify_update_result(Err(crate::updater::UpdateError::Message(
            "boom".to_string(),
        ))) {
            UpdatePrompt::Failed(message) => assert_eq!(message, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
