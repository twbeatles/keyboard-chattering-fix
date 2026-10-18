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

use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc, Mutex, OnceLock, PoisonError,
    },
};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{
            GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        },
    },
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use super::sound::play_notification_sound;
use crate::{
    app::state::{Shared, HOOK_ACTIVE, SHARED, START},
    debounce::{Decision, KeyEventKind, SLOTS},
};

/// `dwExtraInfo` of events this program sends itself ("KCFX").
pub(crate) const INJECT_MARK: usize = 0x4B43_4658;

/// Keyboard events seen by the hook (all of them, filtered or not), for
/// the window's watchdog.
pub(crate) static HOOK_EVENTS: AtomicU64 = AtomicU64::new(0);
/// Watchdog probe: a marked release of this unused key code. The hook
/// swallows it and counts it in [`PROBES_SEEN`].
pub(crate) const PROBE_VK: u16 = 0xFF;
pub(crate) static PROBES_SEEN: AtomicU64 = AtomicU64::new(0);

/// One event to send with `SendInput`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Injection {
    /// Timing slot: virtual key, plus 256 for the extended variant.
    pub slot: u16,
    pub down: bool,
    pub scan: u16,
}

/// Fixed-size queue in `Shared`, so the hook queues without allocating.
#[derive(Default)]
pub(crate) struct InjectQueue {
    items: [Injection; 32],
    len: usize,
}

impl InjectQueue {
    /// Queue one event; a full queue drops it (32 releases at once cannot
    /// happen with real hands).
    pub(crate) fn push(&mut self, item: Injection) {
        if self.len < self.items.len() {
            self.items[self.len] = item;
            self.len += 1;
        }
    }

    fn take(&mut self) -> Vec<Injection> {
        let out = self.items[..self.len].to_vec();
        self.len = 0;
        out
    }
}

thread_local! {
    /// The hook thread's release-delay timer (0 = not running).
    static DEFER_TIMER: Cell<usize> = const { Cell::new(0) };
}

/// Arm the release-delay timer on the hook thread (the caller's thread).
unsafe fn ensure_defer_timer() {
    DEFER_TIMER.with(|t| {
        if t.get() == 0 {
            t.set(SetTimer(None, 0, USER_TIMER_MINIMUM, None));
        }
    });
}

/// Ask the hook thread to drain the injection queue (from the hook itself).
unsafe fn wake_injector() {
    let id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        let _ = PostThreadMessageW(id, WM_KCFIX_INJECT, WPARAM(0), LPARAM(0));
    }
}

/// Carry out a decision for a blocked or held-back event. `slot` and `scan`
/// describe the current event. Returns whether the event is dropped.
unsafe fn apply(
    g: &mut Shared,
    decision: Decision,
    slot: usize,
    scan: u16,
    kind: KeyEventKind,
) -> bool {
    let drop_with_sound = |g: &Shared| {
        if g.play_sound && kind == KeyEventKind::Down {
            play_notification_sound(g.sound_file.as_ref().map(|s| s.wide.as_slice()));
        }
    };
    match decision {
        Decision::Pass => false,
        Decision::Block => {
            drop_with_sound(g);
            true
        }
        Decision::Defer => {
            g.pending_scan[slot] = scan;
            ensure_defer_timer();
            true
        }
        Decision::Flush { then_pass } => {
            g.inject.push(Injection {
                slot: slot as u16,
                down: false,
                scan: g.pending_scan[slot],
            });
            if then_pass {
                g.inject.push(Injection {
                    slot: slot as u16,
                    down: true,
                    scan,
                });
            } else {
                drop_with_sound(g);
            }
            wake_injector();
            true
        }
    }
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode >= 0 {
        let kind = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyEventKind::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(KeyEventKind::Up),
            _ => None,
        };
        if let Some(kind) = kind {
            HOOK_EVENTS.fetch_add(1, Ordering::Relaxed);
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Our own re-sent events were decided when they first arrived;
            // the watchdog's probe proves the hook alive and goes no further.
            if kb.dwExtraInfo == INJECT_MARK {
                if kb.vkCode == u32::from(PROBE_VK) {
                    PROBES_SEEN.fetch_add(1, Ordering::Relaxed);
                    return LRESULT(1);
                }
                return CallNextHookEx(None, ncode, wparam, lparam);
            }
            if let (Some(shared), Some(start)) = (SHARED.get(), START.get()) {
                // A panic here would abort the process (it unwinds into the
                // OS), so a poisoned lock keeps filtering with its data.
                let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);
                let vk = kb.vkCode as usize;
                let extended = (kb.flags & LLKHF_EXTENDED).0 != 0;
                let slot = crate::debounce::slot_of(vk, extended);

                // 1. Cleaning mode: presses dropped, Enter ends it.
                if g.debouncer.cleaning_mode {
                    let d = g
                        .debouncer
                        .decide_event(vk, extended, kind, start.elapsed());
                    if d == Decision::Pass {
                        return CallNextHookEx(None, ncode, wparam, lparam);
                    }
                    return LRESULT(1);
                }

                // 2. Never touch synthesized input if exclude_injected is enabled
                if g.exclude_injected && (kb.flags & LLKHF_INJECTED).0 != 0 {
                    return CallNextHookEx(None, ncode, wparam, lparam);
                }

                // 3. Debounce decision (not while paused for a foreground app)
                if g.enabled && !g.suspended {
                    let d = g
                        .debouncer
                        .decide_event(vk, extended, kind, start.elapsed());
                    if apply(&mut g, d, slot, kb.scanCode as u16, kind) {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// Virtual-key code and direction of a mouse button message; `None` for
/// movement, the wheel, and anything else that is not a button.
fn mouse_button(msg: u32, mouse_data: u32) -> Option<(usize, KeyEventKind)> {
    use KeyEventKind::{Down, Up};
    Some(match msg {
        WM_LBUTTONDOWN => (0x01, Down),
        WM_LBUTTONUP => (0x01, Up),
        WM_RBUTTONDOWN => (0x02, Down),
        WM_RBUTTONUP => (0x02, Up),
        WM_MBUTTONDOWN => (0x04, Down),
        WM_MBUTTONUP => (0x04, Up),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let vk = match (mouse_data >> 16) as u16 {
                XBUTTON1 => 0x05,
                XBUTTON2 => 0x06,
                _ => return None,
            };
            (vk, if msg == WM_XBUTTONDOWN { Down } else { Up })
        }
        _ => return None,
    })
}

unsafe extern "system" fn mouse_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Movement arrives hundreds of times a second: it is classified here
    // and passed on before the shared lock is ever touched.
    if ncode >= 0 {
        let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        if let Some((vk, kind)) = mouse_button(wparam.0 as u32, ms.mouseData) {
            if ms.dwExtraInfo == INJECT_MARK {
                return CallNextHookEx(None, ncode, wparam, lparam);
            }
            if let (Some(shared), Some(start)) = (SHARED.get(), START.get()) {
                let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);
                // Cleaning mode keeps the mouse working: it is how the user
                // clicks "Finish cleaning".
                let skip = g.debouncer.cleaning_mode
                    || !g.enabled
                    || g.suspended
                    || (g.exclude_injected && ms.flags & LLMHF_INJECTED != 0);
                if !skip {
                    let d = g.debouncer.decide_event(vk, false, kind, start.elapsed());
                    if apply(&mut g, d, vk, 0, kind) {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// The `SendInput` record for one queued event.
fn input_for(item: Injection) -> INPUT {
    let vk = item.slot & 0xFF;
    let mouse = |flags: MOUSE_EVENT_FLAGS, data: u32| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                mouseData: data,
                dwFlags: flags,
                dwExtraInfo: INJECT_MARK,
                ..Default::default()
            },
        },
    };
    let (down, up) = (item.down, !item.down);
    match vk {
        0x01 => mouse(
            if down {
                MOUSEEVENTF_LEFTDOWN
            } else {
                MOUSEEVENTF_LEFTUP
            },
            0,
        ),
        0x02 => mouse(
            if down {
                MOUSEEVENTF_RIGHTDOWN
            } else {
                MOUSEEVENTF_RIGHTUP
            },
            0,
        ),
        0x04 => mouse(
            if down {
                MOUSEEVENTF_MIDDLEDOWN
            } else {
                MOUSEEVENTF_MIDDLEUP
            },
            0,
        ),
        0x05 | 0x06 => mouse(
            if down {
                MOUSEEVENTF_XDOWN
            } else {
                MOUSEEVENTF_XUP
            },
            if vk == 0x05 {
                u32::from(XBUTTON1)
            } else {
                u32::from(XBUTTON2)
            },
        ),
        _ => {
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if up {
                flags |= KEYEVENTF_KEYUP;
            }
            if item.slot >= 256 {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: item.scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: INJECT_MARK,
                    },
                },
            }
        }
    }
}

fn send(items: &[Injection]) {
    if items.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = items.iter().copied().map(input_for).collect();
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Thread that only calls `SendInput`, so the hook thread is never inside
/// `SendInput` while its own hooks must be serviced.
fn injector() -> &'static Mutex<mpsc::Sender<Vec<Injection>>> {
    static TX: OnceLock<Mutex<mpsc::Sender<Vec<Injection>>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Vec<Injection>>();
        let _ = std::thread::Builder::new()
            .name("kcfix-injector".into())
            .spawn(move || {
                for batch in rx {
                    send(&batch);
                }
            });
        Mutex::new(tx)
    })
}

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
unsafe fn drain(flush_all: bool) -> Vec<Injection> {
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

/// Thread messages for the hook thread.
/// `wparam` 1 installs the mouse hook, 0 removes it.
const WM_KCFIX_MOUSE: u32 = WM_APP + 0x40;
/// Something was queued for `SendInput`.
const WM_KCFIX_INJECT: u32 = WM_APP + 0x41;
/// Win32 id of the running hook thread (0 when none).
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Generation of the hook thread: claimed on every spawn, retired on every
/// [`HookThread::abandon`]. A thread that finds itself stale at exit
/// (superseded by [`restart`]) must not flush releases the new thread owns
/// now (ISSUE-001). Retiring up front (rather than only claiming in the new
/// thread) closes the race where the old thread exits before the new spawn.
static HOOK_GEN: AtomicU64 = AtomicU64::new(0);
/// What the user chose, so a restarted hook thread gets it back.
static MOUSE_WANTED: AtomicBool = AtomicBool::new(false);
/// The hook thread of this process (see [`start`]).
static HOOK: Mutex<Option<HookThread>> = Mutex::new(None);

/// Turn mouse-button filtering on or off. The hook thread installs or
/// removes its `WH_MOUSE_LL` hook; remembered for a restarted thread.
pub(crate) fn set_mouse_filter(on: bool) {
    MOUSE_WANTED.store(on, Ordering::Relaxed);
    let id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, WM_KCFIX_MOUSE, WPARAM(on as usize), LPARAM(0));
        }
    }
}

/// Start the hook thread for the app. Publishes `HOOK_ACTIVE`.
pub(crate) fn start() -> bool {
    let thread = spawn_hook_thread();
    let ok = thread.is_some();
    *HOOK.lock().unwrap_or_else(PoisonError::into_inner) = thread;
    HOOK_ACTIVE.store(ok, Ordering::Relaxed);
    if ok && MOUSE_WANTED.load(Ordering::Relaxed) {
        set_mouse_filter(true);
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

    fn quit(&self) {
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
    fn abandon(self) {
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
    let gen = HOOK_GEN.fetch_add(1, Ordering::AcqRel) + 1;
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
                            } else if !on {
                                if let Some(h) = mouse.take() {
                                    let _ = UnhookWindowsHookEx(h);
                                }
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
            if gen == HOOK_GEN.load(Ordering::Acquire) {
                send(&drain(true));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that own the process-wide hook state (`HOOK`,
    /// `HOOK_THREAD_ID`, `SHARED`): they must not interleave.
    static HOOK_SERIAL: Mutex<()> = Mutex::new(());

    /// Process-wide filter state for hook-lifecycle tests. Protection stays
    /// off so a running hook keeps forwarding everything, like the
    /// uninitialized state other tests assume.
    fn ensure_test_shared() {
        use std::time::Instant;
        START.get_or_init(Instant::now);
        SHARED.get_or_init(|| {
            Mutex::new(Shared {
                debouncer: crate::debounce::Debouncer::new(50),
                enabled: false,
                play_sound: false,
                sound_file: None,
                exclude_injected: true,
                suspended: false,
                pending_scan: [0; SLOTS],
                inject: Default::default(),
            })
        });
    }

    /// The hook runs on its own thread, never on the caller's (UI) thread,
    /// and stops cleanly. Installing a LL hook needs no window and does not
    /// block other input while the test runs (it forwards everything:
    /// protection is off in the test state, if any).
    #[test]
    fn hook_runs_on_a_dedicated_thread_and_stops() {
        let _serial = HOOK_SERIAL.lock().unwrap();
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
        // The mouse hook comes and goes on the same thread; stopping with
        // it installed must still unhook and join.
        set_mouse_filter(true);
        set_mouse_filter(false);
        set_mouse_filter(true);
        hook.stop();
        assert_eq!(HOOK_THREAD_ID.load(Ordering::Acquire), 0);
        set_mouse_filter(false);
    }

    /// A restart hands in-flight deferred releases to the new thread instead
    /// of flushing them early, and leaves exactly one hook thread behind
    /// (ISSUE-001).
    #[test]
    fn restart_hands_over_pending_releases_to_one_thread() {
        let _serial = HOOK_SERIAL.lock().unwrap();
        ensure_test_shared();
        set_mouse_filter(false);
        if !start() {
            stop();
            eprintln!("skipped: no interactive desktop for a LL keyboard hook");
            return;
        }
        // One release held back with a long delay, via the filter API.
        // (0x41 is this test's own slot; see the flush test's note.)
        let now = START.get().unwrap().elapsed();
        {
            let mut g = SHARED
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            g.debouncer.set_release_defer_ms(60_000);
            assert_eq!(
                g.debouncer
                    .decide_event(0x41, false, KeyEventKind::Down, now),
                Decision::Pass
            );
            assert_eq!(
                g.debouncer.decide_event(0x41, false, KeyEventKind::Up, now),
                Decision::Defer
            );
        }
        let old_gen = HOOK_GEN.load(Ordering::Acquire);
        assert!(restart());
        // Retire (abandon) + claim (spawn) moves the generation forward.
        assert!(HOOK_GEN.load(Ordering::Acquire) > old_gen);
        // The stale thread's exit flush must not have eaten the handoff.
        {
            let mut g = SHARED
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            assert!(g.debouncer.has_pending_releases());
            g.debouncer.set_release_defer_ms(0);
        }
        assert_ne!(HOOK_THREAD_ID.load(Ordering::Acquire), 0);
        stop();
        assert_eq!(HOOK_THREAD_ID.load(Ordering::Acquire), 0);
    }

    /// Cleaning-mode entry flushes held-back releases at once, so no key
    /// looks stuck down when input freezes (ISSUE-002). The stray key-up
    /// below goes through the real injector; a release without a press is
    /// ignored by applications.
    #[test]
    fn flush_deferred_releases_empties_pending() {
        let _serial = HOOK_SERIAL.lock().unwrap();
        ensure_test_shared();
        // Own virtual key: the process-wide Debouncer is shared with the
        // other lifecycle tests, and timing slots must not overlap.
        let now = START.get().unwrap().elapsed();
        {
            let mut g = SHARED
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            g.debouncer.set_release_defer_ms(60_000);
            assert_eq!(
                g.debouncer
                    .decide_event(0x42, false, KeyEventKind::Down, now),
                Decision::Pass
            );
            assert_eq!(
                g.debouncer.decide_event(0x42, false, KeyEventKind::Up, now),
                Decision::Defer
            );
            assert!(g.debouncer.has_pending_releases());
        }
        flush_deferred_releases();
        {
            let mut g = SHARED
                .get()
                .unwrap()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            assert!(!g.debouncer.has_pending_releases());
            g.debouncer.set_release_defer_ms(0);
        }
    }

    /// Concurrent restarts never panic and leave one owned thread behind,
    /// which still stops cleanly (ISSUE-001).
    #[test]
    fn concurrent_restarts_leave_one_hook_thread() {
        let _serial = HOOK_SERIAL.lock().unwrap();
        ensure_test_shared();
        set_mouse_filter(false);
        if !start() {
            stop();
            eprintln!("skipped: no interactive desktop for a LL keyboard hook");
            return;
        }
        let (first, second) = std::thread::scope(|s| {
            let a = s.spawn(restart);
            let b = s.spawn(restart);
            (a.join().unwrap(), b.join().unwrap())
        });
        assert!(first && second);
        stop();
        assert_eq!(HOOK_THREAD_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn mouse_buttons_map_to_their_virtual_keys() {
        use KeyEventKind::{Down, Up};
        assert_eq!(mouse_button(WM_LBUTTONDOWN, 0), Some((0x01, Down)));
        assert_eq!(mouse_button(WM_RBUTTONUP, 0), Some((0x02, Up)));
        assert_eq!(mouse_button(WM_MBUTTONDOWN, 0), Some((0x04, Down)));
        // X buttons carry which one in the high word of mouseData.
        assert_eq!(mouse_button(WM_XBUTTONDOWN, 1 << 16), Some((0x05, Down)));
        assert_eq!(mouse_button(WM_XBUTTONUP, 2 << 16), Some((0x06, Up)));
        assert_eq!(mouse_button(WM_XBUTTONDOWN, 3 << 16), None);
        // Movement and the wheel never reach the filter.
        assert_eq!(mouse_button(WM_MOUSEMOVE, 0), None);
        assert_eq!(mouse_button(WM_MOUSEWHEEL, 120 << 16), None);
    }

    #[test]
    fn injected_events_carry_the_mark_and_the_right_flags() {
        let up = input_for(Injection {
            slot: 0x10D,
            down: false,
            scan: 0x1C,
        });
        assert_eq!(up.r#type, INPUT_KEYBOARD);
        let ki = unsafe { up.Anonymous.ki };
        assert_eq!(ki.wVk, VIRTUAL_KEY(0x0D));
        assert_eq!(ki.wScan, 0x1C);
        assert_eq!(ki.dwFlags, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY);
        assert_eq!(ki.dwExtraInfo, INJECT_MARK);

        let x2 = input_for(Injection {
            slot: 0x06,
            down: true,
            scan: 0,
        });
        assert_eq!(x2.r#type, INPUT_MOUSE);
        let mi = unsafe { x2.Anonymous.mi };
        assert_eq!(mi.dwFlags, MOUSEEVENTF_XDOWN);
        assert_eq!(mi.mouseData, u32::from(XBUTTON2));
        assert_eq!(mi.dwExtraInfo, INJECT_MARK);
    }

    #[test]
    fn inject_queue_is_bounded_and_drains() {
        let mut q = InjectQueue::default();
        for i in 0..40 {
            q.push(Injection {
                slot: i,
                down: false,
                scan: 0,
            });
        }
        let items = q.take();
        assert_eq!(items.len(), 32);
        assert_eq!(items[31].slot, 31);
        assert!(q.take().is_empty());
    }
}
