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

use std::sync::{Mutex, OnceLock, mpsc};

use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

use super::queue::{INJECT_MARK, Injection};

/// The `SendInput` record for one queued event.
pub(super) fn input_for(item: Injection) -> INPUT {
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

/// Send queued events; returns how many the system accepted.
pub(super) fn send(items: &[Injection]) -> usize {
    if items.is_empty() {
        return 0;
    }
    let inputs: Vec<INPUT> = items.iter().copied().map(input_for).collect();
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize }
}

/// Attempts of [`send_retrying`]: about 3 s in all.
pub(super) const SEND_ATTEMPTS: u32 = 30;
pub(super) const SEND_RETRY_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// [`send`] until every event got through. `SendInput` refuses input while
/// the secure desktop (UAC prompt, Ctrl+Alt+Del) is up; a held-back release
/// lost there would leave its key down for the system until pressed again.
/// Injector thread only: it may sleep.
pub(super) fn send_retrying(items: &[Injection]) {
    let sent = retry_send(items, send, SEND_ATTEMPTS, || {
        std::thread::sleep(SEND_RETRY_WAIT)
    });
    if sent < items.len() {
        tracing::warn!(
            lost = items.len() - sent,
            "SendInput kept refusing held-back events"
        );
    }
}

/// Retry loop of [`send_retrying`]: each attempt sends what is still
/// missing, waiting between attempts. Returns how many got through.
pub(super) fn retry_send(
    items: &[Injection],
    mut try_send: impl FnMut(&[Injection]) -> usize,
    attempts: u32,
    wait: impl Fn(),
) -> usize {
    let mut sent = 0;
    for attempt in 1..=attempts {
        sent += try_send(&items[sent..]).min(items.len() - sent);
        if sent >= items.len() {
            break;
        }
        if attempt < attempts {
            wait();
        }
    }
    sent
}

/// Thread that only calls `SendInput`, so the hook thread is never inside
/// `SendInput` while its own hooks must be serviced.
pub(super) fn injector() -> &'static Mutex<mpsc::Sender<Vec<Injection>>> {
    static TX: OnceLock<Mutex<mpsc::Sender<Vec<Injection>>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Vec<Injection>>();
        let _ = std::thread::Builder::new()
            .name("kcfix-injector".into())
            .spawn(move || {
                for batch in rx {
                    send_retrying(&batch);
                }
            });
        Mutex::new(tx)
    })
}
