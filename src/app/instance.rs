//! Single-instance guard (ISSUE-001).
//!
//! Only one process may own the keyboard hook. CLI one-shots (`--version`,
//! `--check-update`) run before this and are exempt on purpose.

/// Guard for the process-wide single-instance mutex.
#[cfg(windows)]
pub(crate) struct InstanceGuard {
    handle: Option<windows::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

/// Claim the single-instance mutex. Returns `None` when another instance
/// already holds it.
#[cfg(windows)]
pub(crate) fn try_acquire_single_instance() -> Option<InstanceGuard> {
    try_acquire_named("Local\\KeyboardChatteringFix_SingleInstance")
}

/// Claim the named mutex; split out so tests use their own name and never
/// collide with a real running app.
#[cfg(windows)]
fn try_acquire_named(name: &str) -> Option<InstanceGuard> {
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        },
    };
    unsafe {
        let name = HSTRING::from(name);
        match CreateMutexW(None, false, &name) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                    None
                } else {
                    Some(InstanceGuard {
                        handle: Some(handle),
                    })
                }
            }
            // Fail open: a broken mutex must never brick the filter.
            Err(_) => Some(InstanceGuard { handle: None }),
        }
    }
}

/// Bring the already-running instance's window to the front (best effort).
#[cfg(windows)]
pub(crate) fn bring_existing_to_front() {
    use windows::{core::w, Win32::UI::WindowsAndMessaging::*};
    unsafe {
        // Must match the class registered in `ui::create_window`.
        if let Ok(hwnd) = FindWindowW(w!("KeyboardChatteringFixMain"), None) {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Tell the user a previous instance is already filtering input.
#[cfg(windows)]
pub(crate) fn show_already_running(language: crate::i18n::Language) {
    use windows::{
        core::HSTRING,
        Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK},
    };
    let s = crate::i18n::strings(language);
    let text = HSTRING::from(s.already_running);
    let title = HSTRING::from(s.title);
    unsafe {
        let _ = MessageBoxW(None, &text, &title, MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(test)]
mod tests {
    /// Second acquisition fails while the first guard is alive, so a double
    /// launch can never install two hooks (ISSUE-001). Uses a per-test name,
    /// so it also passes next to a live tray app.
    #[test]
    #[cfg(windows)]
    fn second_acquisition_fails_while_first_held() {
        let name = format!("Local\\kcfix_test_instance_{}", std::process::id());
        let first = super::try_acquire_named(&name).expect("first acquisition");
        assert!(super::try_acquire_named(&name).is_none());
        // Released with the guard: a later launch may start again.
        drop(first);
        assert!(super::try_acquire_named(&name).is_some());
    }
}
