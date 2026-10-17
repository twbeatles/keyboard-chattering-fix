//! Control text helpers and full-window retranslation.

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{Controls::EM_SETCUEBANNER, WindowsAndMessaging::*},
    },
};

use super::{history, ids::*, layout::SPECS, menu, state::current_lang, stats, tray};
use crate::{
    config,
    i18n::{fill, strings, Language},
};

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) unsafe fn set_text(parent: HWND, id: i32, s: &str) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        let v = to_wide(s);
        let _ = SetWindowTextW(h, PCWSTR(v.as_ptr()));
    }
}

/// Current text of a child control, trimmed.
pub(crate) fn get_text(parent: HWND, id: i32) -> String {
    unsafe {
        if let Ok(h) = GetDlgItem(Some(parent), id) {
            let mut buf = [0u16; 1024];
            let len = GetWindowTextW(h, &mut buf) as usize;
            return String::from_utf16_lossy(&buf[..len.min(buf.len())])
                .trim()
                .to_string();
        }
    }
    String::new()
}

/// Replace a combo box's items and select `sel` (clamped to the list).
pub(crate) unsafe fn fill_combo(parent: HWND, id: i32, items: &[&str], sel: usize) {
    let Ok(combo) = GetDlgItem(Some(parent), id) else {
        return;
    };
    let _ = SendMessageW(combo, CB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
    for item in items {
        let v = to_wide(item);
        SendMessageW(
            combo,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(v.as_ptr() as isize)),
        );
    }
    let sel = sel.min(items.len().saturating_sub(1));
    SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(sel)), Some(LPARAM(0)));
}

/// Selected index of a combo box (0 when nothing is selected).
pub(crate) unsafe fn combo_sel(parent: HWND, id: i32) -> usize {
    GetDlgItem(Some(parent), id)
        .map(|h| SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0)
        .ok()
        .filter(|&i| i >= 0)
        .map_or(0, |i| i as usize)
}

unsafe fn set_cue(parent: HWND, id: i32, cue: &str) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        let v = to_wide(cue);
        SendMessageW(
            h,
            EM_SETCUEBANNER,
            Some(WPARAM(0)),
            Some(LPARAM(v.as_ptr() as isize)),
        );
    }
}

/// Footer text depends on the close-to-tray choice.
pub(crate) unsafe fn refresh_footer(hwnd: HWND, close_to_tray: bool) {
    let s = strings(current_lang());
    let template = if close_to_tray {
        s.footer_tray
    } else {
        s.footer_exit
    };
    set_text(hwnd, ID_FOOTER, &fill(template, &[config::VERSION]));
}

/// Re-apply every visible string for the active language.
pub(crate) unsafe fn apply_language(hwnd: HWND) {
    let s = strings(current_lang());
    let caption = to_wide("Keyboard Chatter Fix");
    let _ = SetWindowTextW(hwnd, PCWSTR(caption.as_ptr()));

    for spec in SPECS {
        if let Some(text) = spec.text {
            set_text(hwnd, spec.id, text(s));
        }
    }

    let measure = combo_sel(hwnd, ID_COMBO_MEASURE);
    fill_combo(hwnd, ID_COMBO_MEASURE, s.measure_options, measure);
    let lang = combo_sel(hwnd, ID_COMBO_LANG);
    // "Automatic" first, then every shipped language in its own name.
    let mut options: Vec<&str> = vec![s.language_auto];
    options.extend(Language::ALL.iter().map(|l| l.native_name()));
    fill_combo(hwnd, ID_COMBO_LANG, &options, lang);
    set_cue(hwnd, ID_EDIT_SOUND_FILE, s.sound_file_cue);

    let close_to_tray = super::state::SAVED
        .get()
        .map(|g| g.lock().unwrap().close_to_tray)
        .unwrap_or(true);
    refresh_footer(hwnd, close_to_tray);

    menu::install(hwnd);
    history::apply_language(hwnd);
    stats::invalidate();
    stats::update_stats(hwnd);
    tray::refresh_tip(hwnd);
}
