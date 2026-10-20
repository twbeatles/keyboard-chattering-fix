//! Global hotkeys: toggle protection and start cleaning mode from anywhere.
//!
//! The settings keep the text the user typed (`Ctrl+Alt+F12`); parsing is
//! pure and unit-tested, registration happens on the UI thread at startup
//! and after every save. `WM_HOTKEY` lands in `commands::run`.

use std::sync::Mutex;

use windows::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
        UnregisterHotKey,
    },
};

use super::{
    ids::{HOTKEY_CLEANING, HOTKEY_TOGGLE},
    keys::vk_name,
    state::current_lang,
};
use crate::i18n::{fill, strings};

/// A parsed hotkey: `RegisterHotKey` modifiers, key, and display text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hotkey {
    pub mods: u32,
    pub vk: u8,
    pub label: String,
}

/// Keys that may stand alone (rarely typed): F13–F24, Pause, Scroll Lock.
fn usable_without_modifier(vk: u8) -> bool {
    matches!(vk, 0x7C..=0x87 | 0x13 | 0x91)
}

/// Modifier keys themselves, and mouse buttons, are never the hotkey key.
fn is_plain_key(vk: u8) -> bool {
    !matches!(
        vk,
        0x01..=0x06 | 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5 | 0xE5 | 0xE7
    )
}

/// Parse `Ctrl+Alt+F12` (any case, spaces allowed). `Ok(None)` for an
/// empty text (no hotkey), `Err(())` when it is not a usable hotkey.
pub(crate) fn parse_hotkey(text: &str) -> Result<Option<Hotkey>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut mods = 0u32;
    let mut key: Option<u8> = None;
    for token in text.split('+').map(str::trim) {
        let lower = token.to_ascii_lowercase();
        let bit = match lower.as_str() {
            "ctrl" | "control" => MOD_CONTROL.0,
            "alt" => MOD_ALT.0,
            "shift" => MOD_SHIFT.0,
            "win" | "windows" => MOD_WIN.0,
            "" => return Err(()),
            _ => {
                if key.is_some() {
                    return Err(());
                }
                let vk = (0x07..=0xFEu8)
                    .filter(|&vk| is_plain_key(vk))
                    .find(|&vk| vk_name(vk).eq_ignore_ascii_case(token))
                    .ok_or(())?;
                key = Some(vk);
                continue;
            }
        };
        if mods & bit != 0 {
            return Err(());
        }
        mods |= bit;
    }
    let vk = key.ok_or(())?;
    if mods == 0 && !usable_without_modifier(vk) {
        return Err(());
    }
    let mut parts: Vec<String> = [
        (MOD_CONTROL.0, "Ctrl"),
        (MOD_ALT.0, "Alt"),
        (MOD_SHIFT.0, "Shift"),
        (MOD_WIN.0, "Win"),
    ]
    .iter()
    .filter(|(bit, _)| mods & bit != 0)
    .map(|(_, name)| name.to_string())
    .collect();
    parts.push(vk_name(vk));
    Ok(Some(Hotkey {
        mods,
        vk,
        label: parts.join("+"),
    }))
}

/// Labels of the registered hotkeys, for the menus: (toggle, cleaning).
static ACTIVE: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));

pub(crate) fn toggle_label() -> Option<String> {
    ACTIVE.lock().unwrap().0.clone()
}

pub(crate) fn cleaning_label() -> Option<String> {
    ACTIVE.lock().unwrap().1.clone()
}

/// (Re)register both hotkeys. Returns user-facing problems, one per
/// hotkey that is invalid or taken by another program.
pub(crate) unsafe fn apply(hwnd: HWND, toggle: &str, cleaning: &str) -> Vec<String> {
    unsafe {
        let s = strings(current_lang());
        let mut problems = Vec::new();
        let mut labels = (None, None);
        for (id, text) in [(HOTKEY_TOGGLE, toggle), (HOTKEY_CLEANING, cleaning)] {
            let _ = UnregisterHotKey(Some(hwnd), id);
            match parse_hotkey(text) {
                Ok(None) => {}
                Err(()) => problems.push(fill(s.hotkey_invalid_fmt, &[text.trim()])),
                Ok(Some(hk)) => {
                    let mods = HOT_KEY_MODIFIERS(hk.mods | MOD_NOREPEAT.0);
                    if RegisterHotKey(Some(hwnd), id, mods, u32::from(hk.vk)).is_ok() {
                        if id == HOTKEY_TOGGLE {
                            labels.0 = Some(hk.label);
                        } else {
                            labels.1 = Some(hk.label);
                        }
                    } else {
                        problems.push(fill(s.hotkey_taken_fmt, &[&hk.label]));
                    }
                }
            }
        }
        *ACTIVE.lock().unwrap() = labels;
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_and_key_parse_in_any_order_and_case() {
        let hk = parse_hotkey(" alt + CTRL+f12 ").unwrap().unwrap();
        assert_eq!(hk.mods, MOD_CONTROL.0 | MOD_ALT.0);
        assert_eq!(hk.vk, 0x7B);
        assert_eq!(hk.label, "Ctrl+Alt+F12");
        let hk = parse_hotkey("Win+Shift+Space").unwrap().unwrap();
        assert_eq!((hk.vk, hk.label.as_str()), (0x20, "Shift+Win+Space"));
        assert_eq!(parse_hotkey("Ctrl+Alt+PageDown").unwrap().unwrap().vk, 0x22);
    }

    #[test]
    fn empty_text_means_no_hotkey() {
        assert_eq!(parse_hotkey(""), Ok(None));
        assert_eq!(parse_hotkey("   "), Ok(None));
    }

    #[test]
    fn plain_typing_keys_need_a_modifier() {
        assert_eq!(parse_hotkey("A"), Err(()));
        assert_eq!(parse_hotkey("Space"), Err(()));
        // Keys nobody types by accident may stand alone.
        assert_eq!(parse_hotkey("F13").unwrap().unwrap().mods, 0);
        assert_eq!(parse_hotkey("Pause").unwrap().unwrap().vk, 0x13);
    }

    #[test]
    fn malformed_hotkeys_are_rejected() {
        for bad in [
            "Ctrl+",
            "Ctrl+Alt",
            "Ctrl+A+B",
            "Ctrl+Ctrl+A",
            "Ctrl+Nope",
            "Ctrl+Shift",
            "Ctrl+LShift",
            "Ctrl+Mouse Left",
        ] {
            assert_eq!(parse_hotkey(bad), Err(()), "{bad}");
        }
    }
}
