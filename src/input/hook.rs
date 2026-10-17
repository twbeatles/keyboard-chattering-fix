//! Low-level keyboard hook: classifies each key event and drops chatter.
//!
//! A `WH_KEYBOARD_LL` callback runs on the thread that installed the hook,
//! inside that thread's message loop. Windows silently removes the hook when
//! the callback misses `LowLevelHooksTimeout` (about 1 s), so the hook gets a
//! dedicated thread that does nothing but pump messages (ISSUE-002): a busy
//! or frozen UI thread can no longer delay system-wide input or unhook the
//! filter. The callback only locks shared state and plays an
//! already-validated sound — no allocation, no filesystem I/O.

use std::sync::{mpsc, PoisonError};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{
            GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        },
    },
    UI::WindowsAndMessaging::*,
};

use super::sound::play_notification_sound;
use crate::{
    app::state::{SHARED, START},
    debounce::KeyEventKind,
};

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode >= 0 {
        let kind = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyEventKind::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(KeyEventKind::Up),
            _ => None,
        };
        if let Some(kind) = kind {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if let (Some(shared), Some(start)) = (SHARED.get(), START.get()) {
                // A panic here would abort the process (it unwinds into the
                // OS), so a poisoned lock keeps filtering with its data.
                let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);

                // 1. Cleaning mode: drops everything, or exits on Enter (0x0D)
                if g.debouncer.cleaning_mode {
                    let _ = g
                        .debouncer
                        .decide(kb.vkCode as usize, kind, start.elapsed());
                    return LRESULT(1);
                }

                // 2. Never touch synthesized input if exclude_injected is enabled
                if g.exclude_injected && (kb.flags & LLKHF_INJECTED).0 != 0 {
                    return CallNextHookEx(None, ncode, wparam, lparam);
                }

                // 3. Debounce decision
                if g.enabled
                    && !g
                        .debouncer
                        .decide(kb.vkCode as usize, kind, start.elapsed())
                {
                    if g.play_sound && kind == KeyEventKind::Down {
                        play_notification_sound(g.sound_file.as_ref().map(|s| s.wide.as_slice()));
                    }
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// The running hook thread. Dropping it without [`HookThread::stop`] leaves
/// the hook in place until the process exits (the OS removes it then).
pub(crate) struct HookThread {
    thread_id: u32,
    handle: std::thread::JoinHandle<()>,
}

impl HookThread {
    /// Win32 id of the hook thread (differs from the UI thread's).
    #[cfg(test)]
    pub(crate) fn thread_id(&self) -> u32 {
        self.thread_id
    }

    /// Unhook and join: the thread leaves its loop on `WM_QUIT`.
    pub(crate) fn stop(self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.handle.join();
    }
}

/// Install the process-wide low-level keyboard hook on its own thread.
/// Returns `None` when the hook could not be installed, so the caller can
/// tell the user protection is not active instead of failing silently (G1).
pub(crate) fn spawn_hook_thread() -> Option<HookThread> {
    let (tx, rx) = mpsc::channel::<Option<u32>>();
    let handle = std::thread::Builder::new()
        .name("kcfix-keyboard-hook".into())
        .spawn(move || unsafe {
            // Input latency of every keystroke depends on this thread.
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            // Create the message queue before publishing the thread id, so
            // a WM_QUIT posted right away by `stop` cannot be lost.
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);

            let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
            let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(hinstance), 0)
            else {
                tracing::error!("SetWindowsHookExW(WH_KEYBOARD_LL) failed");
                let _ = tx.send(None);
                return;
            };
            let _ = tx.send(Some(GetCurrentThreadId()));
            // 0 = WM_QUIT, -1 = error: both end the loop.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(hook);
        })
        .ok()?;
    match rx.recv() {
        Ok(Some(thread_id)) => Some(HookThread { thread_id, handle }),
        _ => {
            let _ = handle.join();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hook runs on its own thread, never on the caller's (UI) thread,
    /// and stops cleanly. Installing a LL hook needs no window and does not
    /// block other input while the test runs (it forwards everything:
    /// `SHARED` is never initialized in unit tests).
    #[test]
    fn hook_runs_on_a_dedicated_thread_and_stops() {
        // Service sessions without an interactive desktop may refuse LL
        // hooks; the dedicated-thread contract is only checkable where the
        // hook installs at all.
        let Some(hook) = spawn_hook_thread() else {
            eprintln!("skipped: no interactive desktop for a LL keyboard hook");
            return;
        };
        let caller = unsafe { GetCurrentThreadId() };
        assert_ne!(hook.thread_id(), 0);
        assert_ne!(hook.thread_id(), caller);
        hook.stop();
    }
}
