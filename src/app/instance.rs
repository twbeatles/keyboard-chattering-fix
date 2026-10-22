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

#[cfg(windows)]
const INSTANCE_MUTEX: &str = "Local\\KeyboardChatteringFix_SingleInstance";

/// Claim the single-instance mutex. Returns `None` when another instance
/// already holds it.
#[cfg(windows)]
pub(crate) fn try_acquire_single_instance() -> Option<InstanceGuard> {
    try_acquire_named(INSTANCE_MUTEX)
}

/// Whether opening a named object failed only because it belongs to an
/// elevated process: it exists, we just may not touch it.
#[cfg(windows)]
fn held_by_elevated(e: &windows::core::Error) -> bool {
    e.code() == windows::Win32::Foundation::ERROR_ACCESS_DENIED.to_hresult()
}

/// Claim the named mutex; split out so tests use their own name and never
/// collide with a real running app.
#[cfg(windows)]
fn try_acquire_named(name: &str) -> Option<InstanceGuard> {
    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
        },
        core::HSTRING,
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
            // An instance running as administrator (sign-in task, "Run as
            // administrator") owns a mutex a non-elevated process may not
            // open: that is "already running", not a broken mutex.
            Err(e) if held_by_elevated(&e) => None,
            // Fail open: a broken mutex must never brick the filter.
            Err(_) => Some(InstanceGuard { handle: None }),
        }
    }
}

/// Whether some instance (elevated or not) holds the named mutex, without
/// creating it.
#[cfg(windows)]
fn instance_running_named(name: &str) -> bool {
    use windows::{
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE},
        },
        core::HSTRING,
    };
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(e) => held_by_elevated(&e),
        }
    }
}

/// Wait (bounded) for another instance to appear; returns whether one did.
/// A non-elevated automatic start uses this to let the elevated sign-in
/// task, which may start a little later, win.
#[cfg(windows)]
pub(crate) fn wait_for_other_instance(limit: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    loop {
        if instance_running_named(INSTANCE_MUTEX) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Bring the already-running instance's window to the front (best effort).
#[cfg(windows)]
pub(crate) fn bring_existing_to_front() {
    use windows::{Win32::UI::WindowsAndMessaging::*, core::w};
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
        Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW},
        core::HSTRING,
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

    /// The existence probe sees a held mutex and never creates one itself.
    #[test]
    #[cfg(windows)]
    fn running_probe_sees_holder_without_claiming() {
        let name = format!("Local\\kcfix_test_probe_{}", std::process::id());
        assert!(!super::instance_running_named(&name));
        // Probing did not create it: the first real claim still succeeds.
        let guard = super::try_acquire_named(&name).expect("first acquisition");
        assert!(super::instance_running_named(&name));
        drop(guard);
        assert!(!super::instance_running_named(&name));
    }

    #[test]
    #[cfg(windows)]
    fn only_access_denied_means_held_by_elevated() {
        use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND};
        let denied = windows::core::Error::from_hresult(ERROR_ACCESS_DENIED.to_hresult());
        let missing = windows::core::Error::from_hresult(ERROR_FILE_NOT_FOUND.to_hresult());
        assert!(super::held_by_elevated(&denied));
        assert!(!super::held_by_elevated(&missing));
    }
}
