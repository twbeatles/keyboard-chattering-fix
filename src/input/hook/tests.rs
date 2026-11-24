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

use std::sync::{Mutex, PoisonError, atomic::Ordering};

use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

use crate::{
    app::state::{SHARED, START, Shared},
    debounce::{Decision, KeyEventKind, SLOTS},
};

use super::control::{
    HOOK_GEN, HOOK_THREAD_ID, flush_deferred_releases, restart, set_mouse_filter,
    spawn_hook_thread, start, stop,
};
use super::procs::{mouse_button, wheel_delta};
use super::queue::{INJECT_MARK, InjectQueue, Injection};
use super::send::{input_for, retry_send};

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
            filter_mouse: false,
            filter_wheel: false,
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
fn refused_releases_are_retried_from_where_they_stopped() {
    let items = [
        Injection {
            slot: 0x41,
            down: false,
            scan: 0x1E,
        },
        Injection {
            slot: 0x42,
            down: false,
            scan: 0x30,
        },
    ];
    // Secure desktop for two attempts, then one event per attempt.
    let mut calls = Vec::new();
    let sent = retry_send(
        &items,
        |rest| {
            calls.push(rest.len());
            if calls.len() <= 2 { 0 } else { 1 }
        },
        30,
        || {},
    );
    assert_eq!(sent, 2);
    assert_eq!(calls, [2, 2, 2, 1]);
    // Never accepted: gives up after the attempts, waiting in between.
    let waits = std::cell::Cell::new(0);
    assert_eq!(
        retry_send(&items, |_| 0, 3, || waits.set(waits.get() + 1)),
        0
    );
    assert_eq!(waits.get(), 2);
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
    // Movement and the wheel are not buttons; the wheel has its own path.
    assert_eq!(mouse_button(WM_MOUSEMOVE, 0), None);
    assert_eq!(mouse_button(WM_MOUSEWHEEL, 120 << 16), None);
}

#[test]
fn wheel_delta_reads_the_signed_high_word() {
    use crate::debounce::WheelAxis::{Horizontal, Vertical};
    assert_eq!(wheel_delta(WM_MOUSEWHEEL, 120 << 16), Some((Vertical, 120)));
    assert_eq!(
        wheel_delta(WM_MOUSEWHEEL, (u32::from(-120i16 as u16)) << 16),
        Some((Vertical, -120))
    );
    assert_eq!(
        wheel_delta(WM_MOUSEHWHEEL, 120 << 16),
        Some((Horizontal, 120))
    );
    assert_eq!(wheel_delta(WM_MOUSEMOVE, 120 << 16), None);
    assert_eq!(wheel_delta(WM_LBUTTONDOWN, 0), None);
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
