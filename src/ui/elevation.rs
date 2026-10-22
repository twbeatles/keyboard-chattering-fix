//! Running as administrator, so windows that run as administrator are
//! filtered too (a non-elevated low-level hook never sees their input).
//!
//! - Tools > Run as administrator: start an elevated copy (UAC prompt) that
//!   waits for this process to exit, then exit.
//! - When elevated: Tools > Start as administrator at sign-in, a scheduled
//!   task (`config::set_elevated_task`). Its state is read once on a worker
//!   thread at startup, and changes run on a worker too: the Task Scheduler
//!   service can take a moment, and the UI thread must never wait on it.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_OK, MB_YESNO, PostMessageW, SW_SHOWNORMAL,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    dialog::msg_box,
    foreground::self_elevated,
    ids::{WM_ELEVATED_TASK_DONE, WM_UPDATE_EXIT},
    state::current_lang,
    text::to_wide,
    window::update_exit_nonce,
};
use crate::{
    config,
    i18n::{fill, strings},
};

static TASK_ON: AtomicBool = AtomicBool::new(false);
/// A task change is running on a worker (the menu item ignores clicks).
static BUSY: AtomicBool = AtomicBool::new(false);
/// Outcome of the last task change, read by `on_task_done`.
static OUTCOME: AtomicU8 = AtomicU8::new(0);

/// `OUTCOME` values.
const DONE_ON: u8 = 1;
const DONE_OFF: u8 = 2;
const FAILED: u8 = 3;
const FAILED_OTHER_ACCOUNT: u8 = 4;

fn outcome_code(on: bool, result: Result<(), config::TaskError>) -> u8 {
    match result {
        Ok(()) if on => DONE_ON,
        Ok(()) => DONE_OFF,
        Err(config::TaskError::OtherAccount) => FAILED_OTHER_ACCOUNT,
        Err(config::TaskError::Failed) => FAILED,
    }
}

/// Look up the sign-in task once, off the UI thread (elevated only: a
/// non-elevated process cannot change it anyway). This also repairs a task
/// left pointing at a moved executable and refreshes the marker that
/// non-elevated automatic starts read.
pub(crate) fn init() {
    if self_elevated() {
        std::thread::spawn(|| TASK_ON.store(config::refresh_elevated_task(), Ordering::Relaxed));
    }
}

pub(crate) fn task_on() -> bool {
    TASK_ON.load(Ordering::Relaxed)
}

pub(crate) fn task_busy() -> bool {
    BUSY.load(Ordering::Acquire)
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
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
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

/// Flip the elevated sign-in task on a worker thread. The normal startup
/// entry is left alone: it is the fallback when the task cannot start, and
/// a non-elevated automatic start yields to the elevated one at sign-in.
pub(crate) fn toggle_task(hwnd: HWND) {
    if BUSY.load(Ordering::Acquire) {
        return;
    }
    let on = !task_on();
    if on && !config::exe_in_protected_location() && !confirm_writable_location(hwnd) {
        return;
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let raw = hwnd.0 as usize;
    std::thread::spawn(move || unsafe {
        OUTCOME.store(
            outcome_code(on, config::set_elevated_task(on)),
            Ordering::Release,
        );
        let _ = PostMessageW(
            Some(HWND(raw as *mut _)),
            WM_ELEVATED_TASK_DONE,
            WPARAM(0),
            LPARAM(0),
        );
    });
}

/// The program sits where any program of this account may replace it: an
/// elevated sign-in start would hand that program administrator rights.
/// Say so and let the user decide (default: No).
fn confirm_writable_location(hwnd: HWND) -> bool {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let s = strings(current_lang());
    let text = fill(s.elevated_autostart_writable_fmt, &[&exe]);
    let answer = unsafe {
        msg_box(
            hwnd,
            s.title,
            &text,
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    answer == IDYES
}

/// `WM_ELEVATED_TASK_DONE`: apply the worker's outcome on the UI thread.
/// Ignored unless a change was actually running (anyone can post the id).
pub(crate) unsafe fn on_task_done(hwnd: HWND) {
    if !BUSY.swap(false, Ordering::AcqRel) {
        return;
    }
    let s = strings(current_lang());
    let text = match OUTCOME.load(Ordering::Acquire) {
        DONE_ON => return TASK_ON.store(true, Ordering::Relaxed),
        DONE_OFF => return TASK_ON.store(false, Ordering::Relaxed),
        FAILED_OTHER_ACCOUNT => s.elevated_autostart_other_account,
        _ => s.elevated_autostart_failed,
    };
    unsafe {
        msg_box(hwnd, s.title, text, MB_OK | MB_ICONWARNING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_passes_our_pid() {
        assert_eq!(relaunch_args(4321), "--after-pid 4321");
    }

    #[test]
    fn task_outcomes_keep_the_failure_reason() {
        use config::TaskError;
        assert_eq!(outcome_code(true, Ok(())), DONE_ON);
        assert_eq!(outcome_code(false, Ok(())), DONE_OFF);
        assert_eq!(outcome_code(true, Err(TaskError::Failed)), FAILED);
        assert_eq!(
            outcome_code(true, Err(TaskError::OtherAccount)),
            FAILED_OTHER_ACCOUNT
        );
    }
}
