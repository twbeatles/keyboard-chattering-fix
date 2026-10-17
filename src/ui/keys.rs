//! Virtual-key names and the History tab's key filter.

/// Human-readable name for a virtual-key code.
pub(crate) fn vk_name(vk: u8) -> String {
    match vk {
        // Mouse buttons (only with the mouse filter on).
        0x01 => "Mouse Left".to_string(),
        0x02 => "Mouse Right".to_string(),
        0x04 => "Mouse Middle".to_string(),
        0x05 => "Mouse Back".to_string(),
        0x06 => "Mouse Forward".to_string(),
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x10 => "Shift".to_string(),
        0x11 => "Ctrl".to_string(),
        0x12 => "Alt".to_string(),
        0x13 => "Pause".to_string(),
        0x14 => "CapsLock".to_string(),
        0x15 => "Hangul".to_string(),
        0x19 => "Hanja".to_string(),
        0x1B => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x23 => "End".to_string(),
        0x24 => "Home".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x2C => "PrintScreen".to_string(),
        0x2D => "Insert".to_string(),
        0x2E => "Delete".to_string(),
        0x30..=0x39 | 0x41..=0x5A => (vk as char).to_string(),
        0x5B => "LWin".to_string(),
        0x5C => "RWin".to_string(),
        0x5D => "Apps".to_string(),
        0x5F => "Sleep".to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x6A => "Num*".to_string(),
        0x6B => "Num+".to_string(),
        0x6D => "Num-".to_string(),
        0x6E => "Num.".to_string(),
        0x6F => "Num/".to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x90 => "NumLock".to_string(),
        0x91 => "ScrollLock".to_string(),
        0xA0 => "LShift".to_string(),
        0xA1 => "RShift".to_string(),
        0xA2 => "LCtrl".to_string(),
        0xA3 => "RCtrl".to_string(),
        0xA4 => "LAlt".to_string(),
        0xA5 => "RAlt".to_string(),
        0xA6 => "BrowserBack".to_string(),
        0xA7 => "BrowserForward".to_string(),
        0xA8 => "BrowserRefresh".to_string(),
        0xA9 => "BrowserStop".to_string(),
        0xAA => "BrowserSearch".to_string(),
        0xAB => "BrowserFavorites".to_string(),
        0xAC => "BrowserHome".to_string(),
        0xAD => "VolumeMute".to_string(),
        0xAE => "VolumeDown".to_string(),
        0xAF => "VolumeUp".to_string(),
        0xB0 => "MediaNext".to_string(),
        0xB1 => "MediaPrev".to_string(),
        0xB2 => "MediaStop".to_string(),
        0xB3 => "MediaPlayPause".to_string(),
        0xB4 => "LaunchMail".to_string(),
        0xB5 => "LaunchMedia".to_string(),
        0xB6 => "LaunchApp1".to_string(),
        0xB7 => "LaunchApp2".to_string(),
        0xBA => ";".to_string(),
        0xBB => "=".to_string(),
        0xBC => ",".to_string(),
        0xBD => "-".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xC0 => "`".to_string(),
        0xDB => "[".to_string(),
        0xDC => "\\".to_string(),
        0xDD => "]".to_string(),
        0xDE => "'".to_string(),
        0xE5 => "IME".to_string(),
        _ => format!("0x{vk:02X}"),
    }
}

/// Keys offered in the Keys tab picker: mouse buttons first, then every
/// named key the filter can actually see. The generic Shift/Ctrl/Alt codes
/// are left out (the hook reports the left/right variants) and so are the
/// IME codes, which are never filtered.
pub(crate) fn selectable_vks() -> Vec<u8> {
    (0x01..=0xFEu8)
        .filter(|&vk| !matches!(vk, 0x10 | 0x11 | 0x12 | 0xE5 | 0xE7))
        .filter(|&vk| !vk_name(vk).starts_with("0x"))
        .collect()
}

/// Case-insensitive substring match of `query` against the key name; an
/// empty query matches every key.
pub(crate) fn key_matches(vk: u8, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || vk_name(vk).to_lowercase().contains(&q.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_letters_digits_and_specials() {
        assert_eq!(vk_name(0x41), "A");
        assert_eq!(vk_name(0x39), "9");
        assert_eq!(vk_name(0x20), "Space");
        assert_eq!(vk_name(0x70), "F1");
        assert_eq!(vk_name(0x87), "F24");
        assert_eq!(vk_name(0x63), "Num3");
        assert_eq!(vk_name(0xFF), "0xFF");
    }

    #[test]
    fn korean_ime_keys_have_names_and_are_searchable() {
        // UG1: 한/영 and 한자 used to show as raw hex and could not be found.
        assert_eq!(vk_name(0x15), "Hangul");
        assert_eq!(vk_name(0x19), "Hanja");
        assert_eq!(vk_name(0xE5), "IME");
        assert!(key_matches(0x15, "hang"));
        assert!(!key_matches(0x15, "0x15"));
    }

    #[test]
    fn picker_lists_mouse_buttons_first_and_skips_unseen_codes() {
        let vks = selectable_vks();
        assert_eq!(&vks[..5], &[0x01, 0x02, 0x04, 0x05, 0x06]);
        assert_eq!(vk_name(0x05), "Mouse Back");
        for skipped in [0x03, 0x10, 0x11, 0x12, 0xE5, 0xE7, 0xFF] {
            assert!(!vks.contains(&skipped), "0x{skipped:02X}");
        }
        for wanted in [0x20, 0x41, 0x5A, 0xA0, 0xA5, 0x87] {
            assert!(vks.contains(&wanted), "0x{wanted:02X}");
        }
        assert!(vks.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn key_filter_is_case_insensitive_substring() {
        assert!(key_matches(0x20, ""));
        assert!(key_matches(0x20, "  "));
        assert!(key_matches(0x20, "spa"));
        assert!(key_matches(0x20, "SPACE"));
        assert!(!key_matches(0x41, "space"));
        // "a" matches "A" and also names containing an 'a' (e.g. Space).
        assert!(key_matches(0x41, "a"));
        assert!(key_matches(0x20, "a"));
        assert!(!key_matches(0x42, "a"));
    }
}
