//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::{
    Foundation::HWND,
    UI::{Controls::EM_SETLIMITTEXT, WindowsAndMessaging::*},
};

use super::super::{
    ids::*,
    state::current_lang,
    text::{get_text, set_text},
    widgets::set_toggle,
};
use super::persist::{LiveFingerprint, PERSISTED};
use crate::{
    config::{self, Settings},
    debounce::MeasureFrom,
    i18n::strings,
};

/// Set while the program itself writes into fields, so the resulting
/// `EN_CHANGE` notifications are not mistaken for user edits.
pub(super) static SUPPRESS_DIRTY: AtomicBool = AtomicBool::new(false);
pub(super) static DIRTY: AtomicBool = AtomicBool::new(false);
/// Timer ticks left before the "Saved" confirmation disappears.
pub(super) static SAVED_TICKS: AtomicU32 = AtomicU32::new(0);
/// 500 ms ticks the "Saved" confirmation stays visible.
pub(super) const SAVED_VISIBLE_TICKS: u32 = 6;

/// Parse `text` as `u64` clamped to `[min, max]`; unparsable input keeps
/// `fallback` (the live value), so a typo never resets a setting.
pub(super) fn clamp_u64(text: &str, fallback: u64, min: u64, max: u64) -> u64 {
    text.trim()
        .parse::<u64>()
        .map(|n| n.clamp(min, max))
        .unwrap_or(fallback)
}

pub(super) unsafe fn set_field(hwnd: HWND, id: i32, text: &str) {
    unsafe {
        SUPPRESS_DIRTY.store(true, Ordering::Relaxed);
        set_text(hwnd, id, text);
        SUPPRESS_DIRTY.store(false, Ordering::Relaxed);
    }
}

/// Read a numeric edit, normalize the visible text to the clamped value,
/// and return it.
pub(super) unsafe fn read_bounded_number(
    hwnd: HWND,
    id: i32,
    fallback: u64,
    min: u64,
    max: u64,
) -> u64 {
    unsafe {
        let value = clamp_u64(&get_text(hwnd, id), fallback, min, max);
        set_field(hwnd, id, &value.to_string());
        value
    }
}

pub(super) unsafe fn select(hwnd: HWND, id: i32, index: usize) {
    unsafe {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            SendMessageW(
                h,
                CB_SETCURSEL,
                Some(windows::Win32::Foundation::WPARAM(index)),
                Some(windows::Win32::Foundation::LPARAM(0)),
            );
        }
    }
}

/// Show `init` in every field (startup).
pub(crate) unsafe fn load_into_form(hwnd: HWND, init: &Settings) {
    unsafe {
        set_field(hwnd, ID_EDIT_THRESHOLD, &init.threshold_ms.to_string());
        set_field(hwnd, ID_EDIT_MIN_CHATTER, &init.min_chatter_ms.to_string());
        set_field(hwnd, ID_EDIT_LOG_LIMIT, &init.log_limit.to_string());
        set_field(
            hwnd,
            ID_EDIT_SOUND_FILE,
            init.sound_file.as_deref().unwrap_or(""),
        );
        set_field(hwnd, ID_EDIT_DEFER, &init.release_defer_ms.to_string());
        set_field(hwnd, ID_EDIT_AUTO_APPS, &init.auto_disable_apps);
        set_field(hwnd, ID_EDIT_HOTKEY_TOGGLE, &init.hotkey_toggle);
        set_field(hwnd, ID_EDIT_HOTKEY_CLEAN, &init.hotkey_cleaning);
        for id in [
            ID_EDIT_THRESHOLD,
            ID_EDIT_MIN_CHATTER,
            ID_EDIT_LOG_LIMIT,
            ID_EDIT_DEFER,
        ] {
            if let Ok(h) = GetDlgItem(Some(hwnd), id) {
                SendMessageW(
                    h,
                    EM_SETLIMITTEXT,
                    Some(windows::Win32::Foundation::WPARAM(4)),
                    Some(windows::Win32::Foundation::LPARAM(0)),
                );
            }
        }

        set_toggle(hwnd, ID_TOGGLE_ENABLE, init.enabled);
        set_toggle(hwnd, ID_TOGGLE_SOUND, init.play_sound);
        set_toggle(hwnd, ID_TOGGLE_AUTOSTART, config::autostart_enabled());
        set_toggle(hwnd, ID_TOGGLE_MINIMIZE, init.minimize_on_launch);
        set_toggle(hwnd, ID_TOGGLE_CLOSE_TO_TRAY, init.close_to_tray);
        set_toggle(hwnd, ID_TOGGLE_EXCLUDE_INJECTED, init.exclude_injected);
        set_toggle(hwnd, ID_TOGGLE_RECORD_ALL, init.record_all_keys);
        set_toggle(hwnd, ID_TOGGLE_MOUSE, init.filter_mouse);
        set_toggle(
            hwnd,
            ID_TOGGLE_AUTO_FULLSCREEN,
            init.auto_disable_fullscreen,
        );

        select(
            hwnd,
            ID_COMBO_MEASURE,
            if init.measure_from == MeasureFrom::Press {
                1
            } else {
                0
            },
        );
        select(hwnd, ID_COMBO_LANG, init.language.combo_index());
        // What is on disk now: the autosave only writes once this moves.
        *PERSISTED.lock().unwrap() = Some(LiveFingerprint {
            total: init.total_presses,
            blocked: init.blocked_presses,
            enabled: init.enabled,
        });
        DIRTY.store(false, Ordering::Relaxed);
        set_text(hwnd, ID_SAVE_STATUS, "");
    }
}

/// A settings field changed by the user: say it is not saved yet.
pub(crate) unsafe fn mark_dirty(hwnd: HWND) {
    unsafe {
        if SUPPRESS_DIRTY.load(Ordering::Relaxed) {
            return;
        }
        SAVED_TICKS.store(0, Ordering::Relaxed);
        if !DIRTY.swap(true, Ordering::Relaxed) {
            set_text(hwnd, ID_SAVE_STATUS, strings(current_lang()).status_dirty);
        }
    }
}

/// Timer tick: let the "Saved" confirmation fade out.
pub(crate) unsafe fn tick(hwnd: HWND) {
    unsafe {
        let left = SAVED_TICKS.load(Ordering::Relaxed);
        if left == 0 {
            return;
        }
        SAVED_TICKS.store(left - 1, Ordering::Relaxed);
        if left == 1 && !DIRTY.load(Ordering::Relaxed) {
            set_text(hwnd, ID_SAVE_STATUS, "");
        }
    }
}

/// Retranslate the status line after a language switch.
pub(crate) unsafe fn refresh_status(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let text = if DIRTY.load(Ordering::Relaxed) {
            s.status_dirty
        } else if SAVED_TICKS.load(Ordering::Relaxed) > 0 {
            s.status_saved
        } else {
            ""
        };
        set_text(hwnd, ID_SAVE_STATUS, text);
    }
}
