//! Settings form: read every control, validate, persist, and apply.

use std::sync::atomic::Ordering;

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Graphics::Gdi::InvalidateRect,
        UI::WindowsAndMessaging::*,
    },
};

use super::{
    ids::*,
    state::{current_lang, LANG, SAVED},
    text::{apply_language, get_check, to_wide},
};
use crate::{app::state::SHARED, config, debounce::MeasureFrom, i18n::Language};

/// Read a single-line edit control as trimmed text.
fn read_edit_text(hwnd: HWND, id: i32) -> Option<String> {
    if let Ok(h) = unsafe { GetDlgItem(Some(hwnd), id) } {
        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(h, &mut buf) } as usize;
        return Some(
            String::from_utf16_lossy(&buf[..len.min(buf.len())])
                .trim()
                .to_string(),
        );
    }
    None
}

/// Parse `text` as `u64` clamped to `[min, max]`; unparsable input keeps
/// `fallback` (the live value), so a typo never resets a setting.
fn clamp_u64(text: &str, fallback: u64, min: u64, max: u64) -> u64 {
    text.trim()
        .parse::<u64>()
        .map(|n| n.clamp(min, max))
        .unwrap_or(fallback)
}

/// Read a numeric edit, normalize the visible text to the clamped value,
/// and return it. A missing control keeps `fallback`.
fn read_bounded_number(hwnd: HWND, id: i32, fallback: u64, min: u64, max: u64) -> u64 {
    let value = read_edit_text(hwnd, id)
        .map(|t| clamp_u64(&t, fallback, min, max))
        .unwrap_or(fallback);
    if let Ok(h) = unsafe { GetDlgItem(Some(hwnd), id) } {
        let back = to_wide(&value.to_string());
        unsafe {
            let _ = SetWindowTextW(h, PCWSTR(back.as_ptr()));
        }
    }
    value
}

pub(crate) unsafe fn on_save(hwnd: HWND) {
    // Live fallbacks so a missing control or a typo keeps the running value.
    let (threshold_fb, min_chatter_fb, log_limit_fb) = SHARED
        .get()
        .map(|g| {
            let g = g.lock().unwrap();
            (
                g.debouncer.threshold_ms(),
                g.debouncer.min_chatter_ms(),
                g.debouncer.log_limit as u64,
            )
        })
        .unwrap_or((50, 0, 100));

    let threshold = read_bounded_number(hwnd, ID_EDIT_THRESHOLD, threshold_fb, 1, 1000);
    let min_chatter = read_bounded_number(hwnd, ID_EDIT_MIN_CHATTER, min_chatter_fb, 0, 1000);
    let log_limit = read_bounded_number(hwnd, ID_EDIT_LOG_LIMIT, log_limit_fb, 10, 5000) as usize;

    let sound_file = read_edit_text(hwnd, ID_EDIT_SOUND_FILE).filter(|s| !s.is_empty());

    let enabled = get_check(hwnd, ID_CHECK_ENABLE);
    let sound = get_check(hwnd, ID_CHECK_SOUND);
    let autostart = get_check(hwnd, ID_CHECK_AUTOSTART);
    let minimize = get_check(hwnd, ID_CHECK_MINIMIZE);
    let close_to_tray = get_check(hwnd, ID_CHECK_CLOSE_TO_TRAY);
    let exclude_injected = get_check(hwnd, ID_CHECK_EXCLUDE_INJECTED);
    let record_all = get_check(hwnd, ID_CHECK_RECORD_ALL);

    let measure = if let Ok(h) = GetDlgItem(Some(hwnd), ID_COMBO_MEASURE) {
        let sel = SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
        if sel == 1 {
            MeasureFrom::Press
        } else {
            MeasureFrom::Release
        }
    } else {
        MeasureFrom::Release
    };

    let lang = if let Ok(h) = GetDlgItem(Some(hwnd), ID_COMBO_LANG) {
        let sel = SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
        Language::from_combo_index(if sel >= 0 { sel as usize } else { 0 })
    } else {
        current_lang()
    };

    config::set_autostart(autostart);
    let mut new_settings = config::load();
    new_settings.threshold_ms = threshold;
    new_settings.enabled = enabled;
    new_settings.play_sound = sound;
    new_settings.sound_file = sound_file.clone();
    new_settings.minimize_on_launch = minimize;
    new_settings.close_to_tray = close_to_tray;
    new_settings.record_all_keys = record_all;
    new_settings.measure_from = measure;
    new_settings.language = lang;
    new_settings.min_chatter_ms = min_chatter;
    new_settings.exclude_injected = exclude_injected;
    new_settings.log_limit = log_limit;

    if let Some(g) = SHARED.get() {
        let mut g = g.lock().unwrap();
        g.debouncer.set_threshold_ms(threshold);
        g.debouncer.set_measure_from(measure);
        g.debouncer.set_min_chatter_ms(min_chatter);
        g.debouncer.record_all_keys = record_all;
        g.debouncer.log_limit = log_limit;
        g.enabled = enabled;
        g.play_sound = sound;
        g.exclude_injected = exclude_injected;
        // Validate once here; the hook must never touch the filesystem.
        g.sound_file = config::resolve_sound_file(sound_file.as_deref());
        new_settings.total_presses = g.debouncer.total_presses;
        new_settings.blocked_presses = g.debouncer.blocked_presses;
        new_settings.per_key_presses = g.debouncer.per_key_presses;
        new_settings.per_key_blocked = g.debouncer.per_key_blocked;
    }
    LANG.store(if lang == Language::En { 1 } else { 0 }, Ordering::Relaxed);
    if !config::save(&new_settings) {
        // The dialog still shows the applied in-memory values, so tell the
        // user they did not reach disk (ISSUE-004).
        let failed = crate::i18n::strings(lang);
        super::dialog::msg_box(
            hwnd,
            failed.title,
            failed.save_failed,
            MB_OK | MB_ICONWARNING,
        );
    }
    if let Some(saved) = SAVED.get() {
        *saved.lock().unwrap() = new_settings;
    }
    apply_language(hwnd);
    let _ = InvalidateRect(Some(hwnd), None, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_accepts_value_inside_range() {
        assert_eq!(clamp_u64("80", 50, 1, 1000), 80);
        assert_eq!(clamp_u64("  80  ", 50, 1, 1000), 80);
    }

    #[test]
    fn clamp_pins_to_bounds() {
        assert_eq!(clamp_u64("0", 50, 1, 1000), 1);
        assert_eq!(clamp_u64("5000", 50, 1, 1000), 1000);
        // Zero is a valid minimum for the min-chatter gap (disables it).
        assert_eq!(clamp_u64("0", 9, 0, 1000), 0);
    }

    #[test]
    fn clamp_keeps_fallback_on_garbage() {
        assert_eq!(clamp_u64("", 50, 1, 1000), 50);
        assert_eq!(clamp_u64("abc", 50, 1, 1000), 50);
        assert_eq!(clamp_u64("-3", 50, 1, 1000), 50);
    }
}
