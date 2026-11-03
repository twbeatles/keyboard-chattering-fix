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
    sync::atomic::{AtomicU64, Ordering},
};

use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::*,
};

use super::super::sound::play_notification_sound;
use super::control::{HOOK_THREAD_ID, WM_KCFIX_INJECT};
use crate::{
    app::state::Shared,
    debounce::{Decision, KeyEventKind},
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

    pub(super) fn take(&mut self) -> Vec<Injection> {
        let out = self.items[..self.len].to_vec();
        self.len = 0;
        out
    }
}

thread_local! {
    /// The hook thread's release-delay timer (0 = not running).
    pub(super) static DEFER_TIMER: Cell<usize> = const { Cell::new(0) };
}

/// Arm the release-delay timer on the hook thread (the caller's thread).
pub(super) unsafe fn ensure_defer_timer() {
    unsafe {
        DEFER_TIMER.with(|t| {
            if t.get() == 0 {
                t.set(SetTimer(None, 0, USER_TIMER_MINIMUM, None));
            }
        });
    }
}

/// Ask the hook thread to drain the injection queue (from the hook itself).
pub(super) unsafe fn wake_injector() {
    unsafe {
        let id = HOOK_THREAD_ID.load(Ordering::Acquire);
        if id != 0 {
            let _ = PostThreadMessageW(id, WM_KCFIX_INJECT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Carry out a decision for a blocked or held-back event. `slot` and `scan`
/// describe the current event. Returns whether the event is dropped.
pub(super) unsafe fn apply(
    g: &mut Shared,
    decision: Decision,
    slot: usize,
    scan: u16,
    kind: KeyEventKind,
) -> bool {
    unsafe {
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
}
