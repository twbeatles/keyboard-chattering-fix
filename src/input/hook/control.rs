//! Low-level keyboard hook: classifies each key event and drops chatter.
//!
//! A `WH_KEYBOARD_LL` callback runs on the thread that installed the hook,
//! inside that thread's message loop. Windows silently removes the hook when
//! the callback misses `LowLevelHooksTimeout` (about 1 s), so the hook gets a
//! dedicated thread that does nothing but pump messages (ISSUE-002): a busy
//! or frozen UI thread can no longer delay system-wide input or unhook the
//! filter. The callback only locks shared state, plays an already-validated
//! sound, and (release delay only) arms a thread timer or posts itself a
//! message — no allocation, no filesystem I/O.
//!
//! Mouse buttons are filtered by an optional `WH_MOUSE_LL` hook on the same
//! thread, installed only while the user wants it: a mouse hook is called
//! for every movement, so it costs nothing when it is not installed.
//! Buttons are mapped to their virtual-key codes (`VK_LBUTTON`…), which the
//! keyboard hook never reports, so the one `Debouncer` serves both.
//!
//! Release delay (`iReleaseDeferMs`): a release of a held key is dropped and
//! remembered; a thread timer later sends it again with `SendInput`, marked
//! with [`INJECT_MARK`] so the hook lets it straight through. `SendInput`
//! runs on a separate injector thread, so the hook thread keeps pumping
//! while the injected events travel through the hooks.
//!
//! The thread can be replaced at runtime ([`restart`]): the window's
//! watchdog does this when the OS dropped the hook.

use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    mpsc,
};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{
            GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        },
    },
    UI::WindowsAndMessaging::*,
};

use super::procs::{hook_proc, mouse_proc};
use super::queue::{DEFER_TIMER, Injection, ensure_defer_timer, wake_injector};
use super::send::{injector, send};
use crate::{
    app::state::{HOOK_ACTIVE, SHARED, START},
    debounce::SLOTS,
};

/// Flush releases held back by the release delay right now, from any thread.
///
/// Used on cleaning-mode entry so applications see every key up before input
/// freezes: no key may look stuck down afterwards (ISSUE-002). Delivery goes
/// through the injector thread, never `SendInput` here.
pub(crate) fn flush_deferred_releases() {
    let batch = {
        let (Some(shared), Some(start)) = (SHARED.get(), START.get()) else {
            return;
        };
        let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);
        let mut due = [0u16; SLOTS];
        let n = g.debouncer.due_releases(start.elapsed(), true, &mut due);
        for &slot in &due[..n] {
            let scan = g.pending_scan[slot as usize];
            g.inject.push(Injection {
                slot,
                down: false,
                scan,
            });
        }
        g.inject.take()
    };
    if !batch.is_empty() {
        let tx = injector().lock().unwrap_or_else(PoisonError::into_inner);
        let _ = tx.send(batch);
    }
}

/// Hook thread, outside the hook callbacks: turn due delayed releases into
/// queued events and hand the queue to the injector. `flush_all` also sends
/// releases whose delay has not run out (shutdown).
pub(super) unsafe fn drain(flush_all: bool) -> Vec<Injection> {
    unsafe {
        let (Some(shared), Some(start)) = (SHARED.get(), START.get()) else {
            return Vec::new();
        };
        let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);
        let mut due = [0u16; SLOTS];
        let n = g
            .debouncer
            .due_releases(start.elapsed(), flush_all, &mut due);
        for &slot in &due[..n] {
            let scan = g.pending_scan[slot as usize];
            g.inject.push(Injection {
                slot,
                down: false,
                scan,
            });
        }
        if !g.debouncer.has_pending_releases() {
            DEFER_TIMER.with(|t| {
                let id = t.replace(0);
                if id != 0 {
                    let _ = KillTimer(None, id);
                }
            });
        } else if !flush_all {
            // In-flight delays adopted from a previous thread (see [`start`]):
            // this thread owns no timer yet, so arm one (ISSUE-001).
            ensure_defer_timer();
        }
        g.inject.take()
    }
}

/// Thread messages for the hook thread.
/// `wparam` 1 installs the mouse hook, 0 removes it.
pub(super) const WM_KCFIX_MOUSE: u32 = WM_APP + 0x40;
/// Something was queued for `SendInput`.
pub(super) const WM_KCFIX_INJECT: u32 = WM_APP + 0x41;
/// Win32 id of the running hook thread (0 when none).
pub(super) static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Generation of the hook thread: claimed on every spawn, retired on every
/// [`HookThread::abandon`]. A thread that finds itself stale at exit
/// (superseded by [`restart`]) must not flush releases the new thread owns
/// now (ISSUE-001). Retiring up front (rather than only claiming in the new
/// thread) closes the race where the old thread exits before the new spawn.
pub(super) static HOOK_GEN: AtomicU64 = AtomicU64::new(0);
/// What the user chose, so a restarted hook thread gets it back.
pub(super) static MOUSE_WANTED: AtomicBool = AtomicBool::new(false);
pub(super) static WHEEL_WANTED: AtomicBool = AtomicBool::new(false);

fn mouse_hook_wanted() -> bool {
    MOUSE_WANTED.load(Ordering::Relaxed) || WHEEL_WANTED.load(Ordering::Relaxed)
}

fn post_mouse_hook_want() {
    let id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(
                id,
                WM_KCFIX_MOUSE,
                WPARAM(usize::from(mouse_hook_wanted())),
                LPARAM(0),
            );
        }
    }
}
/// The hook thread of this process (see [`start`]).
pub(super) static HOOK: Mutex<Option<HookThread>> = Mutex::new(None);

/// Turn mouse-button filtering on or off. The hook thread installs or
/// removes its `WH_MOUSE_LL` hook; remembered for a restarted thread.
/// The hook stays installed while either buttons or the wheel want it.
pub(crate) fn set_mouse_filter(on: bool) {
    MOUSE_WANTED.store(on, Ordering::Relaxed);
    post_mouse_hook_want();
}

/// Turn mouse-wheel reversal filtering on or off. Shares the `WH_MOUSE_LL`
/// hook with the button filter.
pub(crate) fn set_wheel_filter(on: bool) {
    WHEEL_WANTED.store(on, Ordering::Relaxed);
    post_mouse_hook_want();
}

/// Start the hook thread for the app. Publishes `HOOK_ACTIVE`.
pub(crate) fn start() -> bool {
    let thread = spawn_hook_thread();
    let ok = thread.is_some();
    *HOOK.lock().unwrap_or_else(PoisonError::into_inner) = thread;
    HOOK_ACTIVE.store(ok, Ordering::Relaxed);
    if ok && mouse_hook_wanted() {
        post_mouse_hook_want();
    }
    if ok {
        // Adopt deferred releases across a restart: the new thread owns no
        // timer yet, so kick it to drain and re-arm (ISSUE-001).
        let pending = SHARED.get().is_some_and(|s| {
            s.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .debouncer
                .has_pending_releases()
        });
        if pending {
            unsafe {
                wake_injector();
            }
        }
    }
    ok
}

/// Stop the app's hook thread (exit): unhook, send held-back releases, join.
pub(crate) fn stop() {
    let thread = HOOK.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(thread) = thread {
        thread.stop();
    }
}

/// Replace the hook thread (the OS dropped its hook). The old thread is
/// told to quit but not waited for: it may be the reason for the restart.
pub(crate) fn restart() -> bool {
    let old = HOOK.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(old) = old {
        old.abandon();
    }
    start()
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

    pub(super) fn quit(&self) {
        let _ =
            HOOK_THREAD_ID.compare_exchange(self.thread_id, 0, Ordering::AcqRel, Ordering::Acquire);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }

    /// Unhook and join: the thread leaves its loop on `WM_QUIT`.
    pub(crate) fn stop(self) {
        self.quit();
        let _ = self.handle.join();
    }

    /// Tell the thread to quit without waiting for it. Retires its
    /// generation first, so even an immediate exit skips the flush that the
    /// replacement thread now owns (ISSUE-001).
    pub(super) fn abandon(self) {
        HOOK_GEN.fetch_add(1, Ordering::AcqRel);
        self.quit();
    }
}

/// Install the process-wide low-level keyboard hook on its own thread.
/// Returns `None` when the hook could not be installed, so the caller can
/// tell the user protection is not active instead of failing silently (G1).
pub(crate) fn spawn_hook_thread() -> Option<HookThread> {
    let (tx, rx) = mpsc::channel::<Option<u32>>();
    // Claimed up front so the thread knows its generation even if the
    // parent never hears back (hook install failure).
    let generation = HOOK_GEN.fetch_add(1, Ordering::AcqRel) + 1;
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
            let mut mouse: Option<HHOOK> = None;
            // 0 = WM_QUIT, -1 = error: both end the loop.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if msg.hwnd.is_invalid() {
                    match msg.message {
                        WM_KCFIX_MOUSE => {
                            let on = msg.wParam.0 != 0;
                            if on && mouse.is_none() {
                                mouse = SetWindowsHookExW(
                                    WH_MOUSE_LL,
                                    Some(mouse_proc),
                                    Some(hinstance),
                                    0,
                                )
                                .inspect_err(|_| {
                                    tracing::error!("SetWindowsHookExW(WH_MOUSE_LL) failed")
                                })
                                .ok();
                            } else if !on && let Some(h) = mouse.take() {
                                let _ = UnhookWindowsHookEx(h);
                            }
                            continue;
                        }
                        WM_KCFIX_INJECT | WM_TIMER => {
                            let batch = drain(false);
                            if !batch.is_empty() {
                                let tx = injector().lock().unwrap_or_else(PoisonError::into_inner);
                                let _ = tx.send(batch);
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Some(h) = mouse {
                let _ = UnhookWindowsHookEx(h);
            }
            let _ = UnhookWindowsHookEx(hook);
            // Unhooked: releases still held back can be sent from here
            // without waiting for this thread to service them. A thread
            // superseded by `restart` skips this: the new thread owns the
            // pending releases now (ISSUE-001).
            if generation == HOOK_GEN.load(Ordering::Acquire) {
                let _ = send(&drain(true));
            }
        })
        .ok()?;
    match rx.recv() {
        Ok(Some(thread_id)) => {
            HOOK_THREAD_ID.store(thread_id, Ordering::Release);
            Some(HookThread { thread_id, handle })
        }
        _ => {
            let _ = handle.join();
            None
        }
    }
}
