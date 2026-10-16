//! Control text and checkbox helpers plus full-window retranslation.

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Controls::{BST_CHECKED, BST_UNCHECKED},
            WindowsAndMessaging::*,
        },
    },
};

use super::{ids::*, state::current_lang, stats::update_stats};
use crate::i18n::strings;

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) unsafe fn set_text(parent: HWND, id: i32, s: &str) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        let v = to_wide(s);
        let _ = SetWindowTextW(h, PCWSTR(v.as_ptr()));
    }
}

pub(crate) unsafe fn set_check(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        let _ = SendMessageW(
            h,
            BM_SETCHECK,
            Some(WPARAM(if on {
                BST_CHECKED.0 as usize
            } else {
                BST_UNCHECKED.0 as usize
            })),
            Some(LPARAM(0)),
        );
    }
}

pub(crate) unsafe fn get_check(parent: HWND, id: i32) -> bool {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        return SendMessageW(h, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0
            == BST_CHECKED.0 as isize;
    }
    false
}

pub(crate) unsafe fn apply_language(hwnd: HWND) {
    let s = strings(current_lang());
    let t = to_wide(s.title);
    let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));

    set_text(hwnd, ID_HEADER_TITLE, s.title);
    set_text(hwnd, ID_HEADER_SUB, s.subtitle);

    set_text(hwnd, ID_CARD1_TITLE, s.card_telemetry);
    set_text(hwnd, ID_STAT_TOTAL_LABEL, s.stat_total);
    set_text(hwnd, ID_STAT_CHATTER_LABEL, s.stat_blocked);
    set_text(hwnd, ID_STAT_RATIO_LABEL, s.stat_ratio);

    set_text(hwnd, ID_CARD2_TITLE, s.card_settings);
    set_text(hwnd, ID_LABEL_THRESHOLD, s.threshold_label);
    set_text(hwnd, ID_LABEL_UNIT, s.threshold_unit);
    set_text(hwnd, ID_CHECK_ENABLE, s.enable_filter);
    set_text(hwnd, ID_CHECK_SOUND, s.play_sound);
    set_text(hwnd, ID_LABEL_SOUND_FILE, s.sound_file_label);
    set_text(hwnd, ID_BTN_SOUND_TEST, s.sound_test);
    set_text(hwnd, ID_CHECK_AUTOSTART, s.autostart);
    set_text(hwnd, ID_CHECK_MINIMIZE, s.minimize_on_launch);
    set_text(hwnd, ID_CHECK_CLOSE_TO_TRAY, s.close_to_tray);
    set_text(hwnd, ID_CHECK_RECORD_ALL, s.record_all_keys);

    set_text(hwnd, ID_CARD3_TITLE, s.card_tools);
    set_text(hwnd, ID_BTN_CLEAN_MODE, s.clean_mode_btn);
    set_text(hwnd, ID_BTN_OPEN_CPL, s.open_cpl_btn);

    set_text(hwnd, ID_CARD4_TITLE, s.card_options);
    set_text(hwnd, ID_LABEL_MEASURE, s.measure_from_label);
    set_text(hwnd, ID_LABEL_LANG, s.language_label);
    set_text(hwnd, ID_LABEL_MIN_CHATTER, s.min_chatter_label);
    set_text(hwnd, ID_LABEL_MIN_UNIT, s.min_chatter_unit);
    set_text(hwnd, ID_CHECK_EXCLUDE_INJECTED, s.exclude_injected);
    set_text(hwnd, ID_LABEL_LOG_LIMIT, s.log_limit_label);
    set_text(hwnd, ID_LABEL_LOG_UNIT, s.log_limit_unit);
    set_text(hwnd, ID_CHECK_RECORD_ALL, s.record_all_keys);

    set_text(hwnd, ID_BTN_SAVE, s.save);
    set_text(hwnd, ID_STATIC_HINT, s.tray_hint);

    if let Ok(combo_m) = GetDlgItem(Some(hwnd), ID_COMBO_MEASURE) {
        let cur = SendMessageW(combo_m, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
        let _ = SendMessageW(combo_m, CB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
        let opt1 = to_wide(s.measure_release);
        let opt2 = to_wide(s.measure_press);
        SendMessageW(
            combo_m,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(opt1.as_ptr() as isize)),
        );
        SendMessageW(
            combo_m,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(opt2.as_ptr() as isize)),
        );
        let sel = if cur >= 0 { cur as usize } else { 0 };
        SendMessageW(combo_m, CB_SETCURSEL, Some(WPARAM(sel)), Some(LPARAM(0)));
    }

    update_stats(hwnd);
}
