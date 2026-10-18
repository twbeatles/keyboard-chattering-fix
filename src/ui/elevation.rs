//! Running as administrator, so windows that run as administrator are
//! filtered too (a non-elevated low-level hook never sees their input).
//!
//! - Tools > Run as administrator: start an elevated copy (UAC prompt) that
//!   waits for this process to exit, then exit.
//! - When elevated: Tools > Start as administrator at sign-in, a scheduled
//!   task (`config::set_elevated_task`). Its state is read once on a worker
//!   thread at startup, because `schtasks` takes a moment.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{PostMessageW, MB_ICONWARNING, MB_OK, SW_SHOWNORMAL},
        },
    },
};

use super::{
    dialog::msg_box, foreground::self_elevated, ids::WM_UPDATE_EXIT, state::current_lang,
    text::to_wide, window::update_exit_nonce,
};
use crate::{config, i18n::strings};

static TASK_ON: AtomicBool = AtomicBool::new(false);

/// Look up the sign-in task once, off the UI thread (elevated only: a
/// non-elevated process cannot change it anyway).
pub(crate) fn init() {
    if self_elevated() {
        std::thread::spawn(|| TASK_ON.store(config::elevated_task_exists(), Ordering::Relaxed));
    }
}

pub(crate) fn task_on() -> bool {
    TASK_ON.load(Ordering::Relaxed)
}

/// Arguments for the elevated copy.
pub(crate) fn relaunch_args(pid: u32) -> String {
    format!("--after-pid {pid}")
}

/// Start an elevated copy and exit once the user approved it. The UAC
/// prompt blocks `ShellExecuteW`, so this runs on a worker thread (with
/// COM, which the shell needs).
pub(crate) fn relaunch_elevated(hwnd: HWND) {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    let raw = hwnd.0 as usize;
    std::thread::spawn(move || unsafe {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let file = to_wide(&exe.to_string_lossy());
        let args = to_wide(&relaunch_args(std::process::id()));
        let result = ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(file.as_ptr()),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if com.is_ok() {
            CoUninitialize();
        }
        // > 32 = started; otherwise the prompt was declined or failed.
        if result.0 as usize > 32 {
            let _ = PostMessageW(
                Some(HWND(raw as *mut _)),
                WM_UPDATE_EXIT,
                WPARAM(update_exit_nonce() as usize),
                LPARAM(0),
            );
        }
    });
}

/// Flip the elevated sign-in task. Turning it on also removes the normal
/// startup entry, so the elevated start is the one that wins at sign-in.
pub(crate) unsafe fn toggle_task(hwnd: HWND) {
    let on = !task_on();
    let ok = config::set_elevated_task(on);
    if ok {
        TASK_ON.store(on, Ordering::Relaxed);
        if on {
            let _ = config::set_autostart(false);
            super::widgets::set_toggle(hwnd, super::ids::ID_TOGGLE_AUTOSTART, false);
        }
    } else {
        let s = strings(current_lang());
        msg_box(
            hwnd,
            s.title,
            s.elevated_autostart_failed,
            MB_OK | MB_ICONWARNING,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_passes_our_pid() {
        assert_eq!(relaunch_args(4321), "--after-pid 4321");
    }
}
