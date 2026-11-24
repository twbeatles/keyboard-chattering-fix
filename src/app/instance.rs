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

/// CLI flags that control the running instance instead of starting a new
/// one. Only protection/cleaning commands are exposed: destructive ones
/// (exit, update) are never accepted over `WM_COPYDATA`, like the
/// `WM_UPDATE_EXIT` nonce rule (SEC-001).
#[cfg(windows)]
pub(crate) fn control_command(arg: &str) -> Option<usize> {
    use crate::ui::ids::{MENU_CLEANING, MENU_PAUSE_15, MENU_PAUSE_60, MENU_RESUME, MENU_TOGGLE};
    match arg {
        "--toggle" => Some(MENU_TOGGLE),
        "--pause-15" => Some(MENU_PAUSE_15),
        "--pause-60" => Some(MENU_PAUSE_60),
        "--resume" => Some(MENU_RESUME),
        "--cleaning" => Some(MENU_CLEANING),
        _ => None,
    }
}

/// Whether `dwData` of a `WM_COPYDATA` message is a command this process
/// accepts from a second invocation. Anything else (exit, update, window
/// messages with side effects) is ignored.
#[cfg(windows)]
pub(crate) fn copydata_command(dw_data: usize) -> Option<usize> {
    use crate::ui::ids::{MENU_CLEANING, MENU_PAUSE_15, MENU_PAUSE_60, MENU_RESUME, MENU_TOGGLE};
    match dw_data {
        MENU_TOGGLE | MENU_PAUSE_15 | MENU_PAUSE_60 | MENU_RESUME | MENU_CLEANING => Some(dw_data),
        _ => None,
    }
}

/// Outcome of forwarding one control command to the running instance.
#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ForwardOutcome {
    /// The running instance acknowledged the command.
    Sent,
    /// No window of this program is running under this account's session.
    NotRunning,
    /// The window exists but did not take the command (another elevation,
    /// or its message loop is stuck).
    Failed,
}

/// Forward `cmd` (a `MENU_*` id from [`control_command`]) to the running
/// instance's window. `class` is the main window class; tests pass a bogus
/// name so they never touch a real tray app (a `--toggle` probe would flip
/// a user's protection).
#[cfg(windows)]
pub(crate) fn forward_control_to(class: &str, cmd: usize) -> ForwardOutcome {
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::{
        Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::*,
        },
        core::HSTRING,
    };
    unsafe {
        let Ok(hwnd) = FindWindowW(&HSTRING::from(class), None) else {
            return ForwardOutcome::NotRunning;
        };
        if hwnd.is_invalid() {
            return ForwardOutcome::NotRunning;
        }
        let data = COPYDATASTRUCT {
            dwData: cmd,
            cbData: 0,
            lpData: std::ptr::null_mut(),
        };
        let mut received = 0usize;
        let rc = SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const _ as isize),
            SMTO_NORMAL,
            5000,
            Some(&mut received),
        );
        if rc.0 != 0 && received == 1 {
            ForwardOutcome::Sent
        } else {
            ForwardOutcome::Failed
        }
    }
}

/// Forward `cmd` to the running instance of this program.
#[cfg(windows)]
pub(crate) fn forward_control(cmd: usize) -> ForwardOutcome {
    // Must match the class registered in `ui::create_window`.
    forward_control_to("KeyboardChatteringFixMain", cmd)
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

    /// Every control flag maps to its menu command; anything else starts
    /// the GUI as usual instead of controlling a running copy.
    #[test]
    #[cfg(windows)]
    fn control_flags_map_to_their_commands() {
        use crate::ui::ids::{
            MENU_CLEANING, MENU_PAUSE_15, MENU_PAUSE_60, MENU_RESUME, MENU_TOGGLE,
        };
        assert_eq!(super::control_command("--toggle"), Some(MENU_TOGGLE));
        assert_eq!(super::control_command("--pause-15"), Some(MENU_PAUSE_15));
        assert_eq!(super::control_command("--pause-60"), Some(MENU_PAUSE_60));
        assert_eq!(super::control_command("--resume"), Some(MENU_RESUME));
        assert_eq!(super::control_command("--cleaning"), Some(MENU_CLEANING));
        for other in [
            "--toggleX",
            "--pause",
            "--exit",
            "--update",
            "",
            "--version",
        ] {
            assert_eq!(super::control_command(other), None, "{other}");
        }
    }

    /// The receiver only runs the allow-listed commands: exit, update and
    /// made-up ids are ignored, like the `WM_UPDATE_EXIT` nonce rule.
    #[test]
    #[cfg(windows)]
    fn copydata_accepts_only_control_commands() {
        use crate::ui::ids::*;
        for cmd in [
            MENU_TOGGLE,
            MENU_PAUSE_15,
            MENU_PAUSE_60,
            MENU_RESUME,
            MENU_CLEANING,
        ] {
            assert_eq!(super::copydata_command(cmd), Some(cmd));
        }
        for denied in [
            MENU_SHOW,
            MENU_EXIT,
            MENU_UPDATE,
            MENU_HIDE,
            MENU_RESET_STATS,
            MENU_RUN_ELEVATED,
            0,
            999_999,
        ] {
            assert_eq!(super::copydata_command(denied), None);
        }
    }

    /// Forwarding to a window class that cannot exist reports "not
    /// running" without touching a real tray app (a `--toggle` probe would
    /// flip a user's protection).
    #[test]
    #[cfg(windows)]
    fn forward_to_a_missing_window_is_not_running() {
        let bogus = format!("kcfix_no_such_window_{}", std::process::id());
        assert_eq!(
            super::forward_control_to(&bogus, super::control_command("--toggle").unwrap()),
            super::ForwardOutcome::NotRunning
        );
    }
}
