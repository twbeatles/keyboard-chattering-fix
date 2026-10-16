//! Low-level keyboard hook: classifies each key event and drops chatter.
//!
//! Runs on the system hook thread; it only locks shared state and plays an
//! already-validated sound — no allocation-heavy work, no filesystem I/O.

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
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
                let mut g = shared.lock().unwrap();

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

/// Install the process-wide low-level keyboard hook.
pub(crate) fn install_hook(hinstance: HINSTANCE) -> HHOOK {
    unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(hinstance), 0).unwrap_or_default()
    }
}
