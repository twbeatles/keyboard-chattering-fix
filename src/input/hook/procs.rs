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

use std::sync::{PoisonError, atomic::Ordering};

use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::*,
};

use super::queue::{HOOK_EVENTS, INJECT_MARK, PROBE_VK, PROBES_SEEN, apply};
use crate::{
    app::state::{SHARED, START},
    debounce::{Decision, KeyEventKind, WheelAxis},
};

pub(super) unsafe extern "system" fn hook_proc(
    ncode: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
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
}

/// Axis and signed tick count of a `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`
/// message (`None` for anything else). The high word of `mouseData` is the
/// signed tick count.
pub(super) fn wheel_delta(msg: u32, mouse_data: u32) -> Option<(WheelAxis, i32)> {
    let axis = match msg {
        WM_MOUSEWHEEL => WheelAxis::Vertical,
        WM_MOUSEHWHEEL => WheelAxis::Horizontal,
        _ => return None,
    };
    Some((axis, ((mouse_data >> 16) as u16 as i16) as i32))
}

/// Virtual-key code and direction of a mouse button message; `None` for
/// movement, the wheels, and anything else that is not a button.
pub(super) fn mouse_button(msg: u32, mouse_data: u32) -> Option<(usize, KeyEventKind)> {
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

pub(super) unsafe extern "system" fn mouse_proc(
    ncode: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
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
                        || !g.filter_mouse
                        || (g.exclude_injected && ms.flags & LLMHF_INJECTED != 0);
                    if !skip {
                        let d = g.debouncer.decide_event(vk, false, kind, start.elapsed());
                        if apply(&mut g, d, vk, 0, kind) {
                            return LRESULT(1);
                        }
                    }
                }
            } else if let Some((axis, delta)) = wheel_delta(wparam.0 as u32, ms.mouseData) {
                if ms.dwExtraInfo == INJECT_MARK {
                    return CallNextHookEx(None, ncode, wparam, lparam);
                }
                if let (Some(shared), Some(start)) = (SHARED.get(), START.get()) {
                    let mut g = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    let skip = g.debouncer.cleaning_mode
                        || !g.enabled
                        || g.suspended
                        || !g.filter_wheel
                        || (g.exclude_injected && ms.flags & LLMHF_INJECTED != 0);
                    if !skip {
                        HOOK_EVENTS.fetch_add(1, Ordering::Relaxed);
                        if !g.debouncer.decide_wheel(delta, start.elapsed(), axis) {
                            if g.play_sound {
                                super::super::sound::play_notification_sound(
                                    g.sound_file.as_ref().map(|s| s.wide.as_slice()),
                                );
                            }
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, ncode, wparam, lparam)
    }
}
