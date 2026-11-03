//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use super::super::ids::*;
use super::kinds::{Kind, Spec, TextFn};
use super::page::{ADVANCED, CLEANING, EVERY, HISTORY, HOME, KEYS, PAGES, SETTINGS};

pub(super) const fn c(
    id: i32,
    kind: Kind,
    scope: u8,
    rect: (i32, i32, i32, i32),
    text: Option<TextFn>,
) -> Spec {
    Spec {
        id,
        kind,
        scope,
        rect,
        text,
        on_card: true,
    }
}

pub(super) const fn w(
    id: i32,
    kind: Kind,
    scope: u8,
    rect: (i32, i32, i32, i32),
    text: Option<TextFn>,
) -> Spec {
    Spec {
        id,
        kind,
        scope,
        rect,
        text,
        on_card: false,
    }
}

/// Creation order is also the Tab-key order.
pub(crate) static SPECS: &[Spec] = &[
    // Header
    w(
        ID_HEADER_TITLE,
        Kind::Title,
        EVERY,
        (24, 12, 300, 28),
        Some(|s| s.title),
    ),
    w(
        ID_HEADER_SUB,
        Kind::Subtitle,
        EVERY,
        (24, 40, 300, 18),
        Some(|s| s.subtitle),
    ),
    w(ID_PILL, Kind::Pill, EVERY, (330, 18, 134, 30), None),
    // Tabs (track painted at TAB_TRACK)
    w(
        ID_TAB_HOME,
        Kind::Tab,
        PAGES,
        (20, 72, 88, 30),
        Some(|s| s.tab_home),
    ),
    w(
        ID_TAB_HISTORY,
        Kind::Tab,
        PAGES,
        (108, 72, 88, 30),
        Some(|s| s.tab_history),
    ),
    w(
        ID_TAB_KEYS,
        Kind::Tab,
        PAGES,
        (196, 72, 88, 30),
        Some(|s| s.tab_keys),
    ),
    w(
        ID_TAB_SETTINGS,
        Kind::Tab,
        PAGES,
        (284, 72, 88, 30),
        Some(|s| s.tab_settings),
    ),
    w(
        ID_TAB_ADVANCED,
        Kind::Tab,
        PAGES,
        (372, 72, 88, 30),
        Some(|s| s.tab_advanced),
    ),
    // Home: protection card
    c(
        ID_TOGGLE_ENABLE,
        Kind::ToggleLarge,
        HOME,
        (32, 130, 416, 28),
        Some(|s| s.enable_filter),
    ),
    c(
        ID_HOME_DESC,
        Kind::Hint,
        HOME,
        (32, 164, 416, 36),
        Some(|s| s.home_desc),
    ),
    // Home: statistics card
    c(
        ID_STATS_TITLE,
        Kind::Section,
        HOME,
        (32, 236, 300, 20),
        Some(|s| s.card_stats),
    ),
    c(
        ID_STAT_TOTAL_LABEL,
        Kind::StatLabel,
        HOME,
        (32, 264, 124, 18),
        Some(|s| s.stat_total),
    ),
    c(
        ID_STAT_TOTAL_VAL,
        Kind::StatValue,
        HOME,
        (32, 282, 124, 32),
        None,
    ),
    c(
        ID_STAT_BLOCKED_LABEL,
        Kind::StatLabel,
        HOME,
        (174, 264, 124, 18),
        Some(|s| s.stat_blocked),
    ),
    c(
        ID_STAT_BLOCKED_VAL,
        Kind::StatValue,
        HOME,
        (174, 282, 124, 32),
        None,
    ),
    c(
        ID_STAT_RATIO_LABEL,
        Kind::StatLabel,
        HOME,
        (316, 264, 124, 18),
        Some(|s| s.stat_ratio),
    ),
    c(
        ID_STAT_RATIO_VAL,
        Kind::StatAccent,
        HOME,
        (316, 282, 124, 32),
        None,
    ),
    c(ID_STAT_LAST, Kind::Detail, HOME, (32, 324, 416, 18), None),
    c(ID_STAT_TOP, Kind::Detail, HOME, (32, 346, 416, 18), None),
    // Home: tools card
    c(
        ID_TOOLS_TITLE,
        Kind::Section,
        HOME,
        (32, 396, 300, 20),
        Some(|s| s.card_tools),
    ),
    c(
        ID_BTN_CLEAN_MODE,
        Kind::Button,
        HOME,
        (32, 424, 132, 30),
        Some(|s| s.clean_mode_btn),
    ),
    c(
        ID_BTN_RESET_STATS,
        Kind::Button,
        HOME,
        (174, 424, 132, 30),
        Some(|s| s.reset_stats_btn),
    ),
    c(
        ID_BTN_OPEN_CPL,
        Kind::Button,
        HOME,
        (316, 424, 132, 30),
        Some(|s| s.open_cpl_btn),
    ),
    w(
        ID_HOME_TIP,
        Kind::Hint,
        HOME,
        (24, 480, 432, 36),
        Some(|s| s.home_tip),
    ),
    w(ID_HOME_NOTE, Kind::Hint, HOME, (24, 524, 432, 52), None),
    // Cleaning overlay
    c(
        ID_CLEAN_TITLE,
        Kind::CleanTitle,
        CLEANING,
        (32, 146, 416, 30),
        Some(|s| s.clean_title),
    ),
    c(
        ID_CLEAN_BODY,
        Kind::CleanBody,
        CLEANING,
        (40, 188, 400, 64),
        Some(|s| s.clean_body),
    ),
    c(
        ID_BTN_CLEAN_END,
        Kind::Primary,
        CLEANING,
        (160, 270, 160, 38),
        Some(|s| s.clean_end_btn),
    ),
    // History
    w(
        ID_HIST_VIEW,
        Kind::Combo,
        HISTORY,
        (16, 118, 160, 240),
        None,
    ),
    w(
        ID_HIST_SEARCH,
        Kind::Edit,
        HISTORY,
        (184, 118, 170, 25),
        None,
    ),
    w(
        ID_BTN_CLEAR_LOG,
        Kind::Button,
        HISTORY,
        (362, 116, 102, 29),
        Some(|s| s.history_clear_btn),
    ),
    w(ID_HIST_LIST, Kind::List, HISTORY, (16, 152, 448, 460), None),
    w(ID_HIST_INFO, Kind::Hint, HISTORY, (16, 620, 448, 36), None),
    // Settings: block window card (the title labels the edit beside it)
    c(
        ID_THRESH_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 130, 130, 20),
        Some(|s| s.card_threshold),
    ),
    c(
        ID_EDIT_THRESHOLD,
        Kind::NumberEdit,
        SETTINGS,
        (166, 127, 64, 25),
        None,
    ),
    c(
        ID_LABEL_UNIT,
        Kind::Label,
        SETTINGS,
        (238, 130, 60, 20),
        Some(|s| s.unit_ms),
    ),
    c(
        ID_THRESH_HINT,
        Kind::Hint,
        SETTINGS,
        (32, 160, 416, 36),
        Some(|s| s.threshold_hint),
    ),
    // Settings: sound card
    c(
        ID_SOUND_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 226, 300, 20),
        Some(|s| s.card_sound),
    ),
    c(
        ID_TOGGLE_SOUND,
        Kind::Toggle,
        SETTINGS,
        (32, 252, 316, 26),
        Some(|s| s.play_sound),
    ),
    c(
        ID_BTN_SOUND_TEST,
        Kind::Button,
        SETTINGS,
        (360, 250, 88, 29),
        Some(|s| s.sound_test),
    ),
    c(
        ID_EDIT_SOUND_FILE,
        Kind::Edit,
        SETTINGS,
        (32, 286, 318, 25),
        None,
    ),
    c(
        ID_BTN_SOUND_BROWSE,
        Kind::Button,
        SETTINGS,
        (360, 284, 88, 29),
        Some(|s| s.sound_browse),
    ),
    // Settings: app card
    c(
        ID_APP_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 343, 300, 20),
        Some(|s| s.card_app),
    ),
    c(
        ID_TOGGLE_AUTOSTART,
        Kind::Toggle,
        SETTINGS,
        (32, 367, 416, 26),
        Some(|s| s.autostart),
    ),
    c(
        ID_TOGGLE_MINIMIZE,
        Kind::Toggle,
        SETTINGS,
        (32, 393, 416, 26),
        Some(|s| s.minimize_on_launch),
    ),
    c(
        ID_TOGGLE_CLOSE_TO_TRAY,
        Kind::Toggle,
        SETTINGS,
        (32, 419, 416, 26),
        Some(|s| s.close_to_tray),
    ),
    c(
        ID_LABEL_LANG,
        Kind::Label,
        SETTINGS,
        (32, 454, 130, 20),
        Some(|s| s.language_label),
    ),
    c(
        ID_COMBO_LANG,
        Kind::Combo,
        SETTINGS,
        (166, 450, 160, 240),
        None,
    ),
    // Settings: automatic pause card
    c(
        ID_AUTO_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 505, 300, 20),
        Some(|s| s.card_auto_pause),
    ),
    c(
        ID_LABEL_AUTO_APPS,
        Kind::Label,
        SETTINGS,
        (32, 532, 130, 20),
        Some(|s| s.auto_apps_label),
    ),
    c(
        ID_EDIT_AUTO_APPS,
        Kind::Edit,
        SETTINGS,
        (166, 529, 282, 25),
        None,
    ),
    c(
        ID_TOGGLE_AUTO_FULLSCREEN,
        Kind::Toggle,
        SETTINGS,
        (32, 560, 416, 26),
        Some(|s| s.auto_fullscreen),
    ),
    // Advanced: detection card
    c(
        ID_DETECT_TITLE,
        Kind::Section,
        ADVANCED,
        (32, 130, 300, 20),
        Some(|s| s.card_detect),
    ),
    c(
        ID_LABEL_MEASURE,
        Kind::Label,
        ADVANCED,
        (32, 160, 130, 20),
        Some(|s| s.measure_from_label),
    ),
    c(
        ID_COMBO_MEASURE,
        Kind::Combo,
        ADVANCED,
        (166, 156, 282, 240),
        None,
    ),
    c(
        ID_LABEL_MIN_CHATTER,
        Kind::Label,
        ADVANCED,
        (32, 192, 130, 20),
        Some(|s| s.min_chatter_label),
    ),
    c(
        ID_EDIT_MIN_CHATTER,
        Kind::NumberEdit,
        ADVANCED,
        (166, 189, 64, 25),
        None,
    ),
    c(
        ID_LABEL_MIN_UNIT,
        Kind::Label,
        ADVANCED,
        (238, 192, 210, 20),
        Some(|s| s.min_chatter_unit),
    ),
    c(
        ID_MIN_HINT,
        Kind::Hint,
        ADVANCED,
        (32, 220, 416, 34),
        Some(|s| s.min_chatter_hint),
    ),
    c(
        ID_LABEL_DEFER,
        Kind::Label,
        ADVANCED,
        (32, 263, 130, 20),
        Some(|s| s.defer_label),
    ),
    c(
        ID_EDIT_DEFER,
        Kind::NumberEdit,
        ADVANCED,
        (166, 260, 64, 25),
        None,
    ),
    c(
        ID_LABEL_DEFER_UNIT,
        Kind::Label,
        ADVANCED,
        (238, 263, 210, 20),
        Some(|s| s.defer_unit),
    ),
    c(
        ID_DEFER_HINT,
        Kind::Hint,
        ADVANCED,
        (32, 291, 416, 34),
        Some(|s| s.defer_hint),
    ),
    c(
        ID_TOGGLE_EXCLUDE_INJECTED,
        Kind::Toggle,
        ADVANCED,
        (32, 330, 416, 26),
        Some(|s| s.exclude_injected),
    ),
    // Advanced: hotkeys card (each field has its own label)
    c(
        ID_LABEL_HOTKEYS,
        Kind::Section,
        ADVANCED,
        (32, 386, 300, 20),
        Some(|s| s.hotkeys_label),
    ),
    c(
        ID_LABEL_HOTKEY_TOGGLE,
        Kind::Label,
        ADVANCED,
        (32, 415, 98, 20),
        Some(|s| s.hotkey_toggle_label),
    ),
    c(
        ID_EDIT_HOTKEY_TOGGLE,
        Kind::Edit,
        ADVANCED,
        (132, 412, 104, 25),
        None,
    ),
    c(
        ID_LABEL_HOTKEY_CLEAN,
        Kind::Label,
        ADVANCED,
        (250, 415, 84, 20),
        Some(|s| s.hotkey_clean_label),
    ),
    c(
        ID_EDIT_HOTKEY_CLEAN,
        Kind::Edit,
        ADVANCED,
        (336, 412, 112, 25),
        None,
    ),
    // Advanced: history card
    c(
        ID_LOG_TITLE,
        Kind::Section,
        ADVANCED,
        (32, 467, 300, 20),
        Some(|s| s.card_log),
    ),
    c(
        ID_TOGGLE_RECORD_ALL,
        Kind::Toggle,
        ADVANCED,
        (32, 491, 416, 26),
        Some(|s| s.record_all_keys),
    ),
    c(
        ID_LABEL_LOG_LIMIT,
        Kind::Label,
        ADVANCED,
        (32, 525, 130, 20),
        Some(|s| s.log_limit_label),
    ),
    c(
        ID_EDIT_LOG_LIMIT,
        Kind::NumberEdit,
        ADVANCED,
        (166, 522, 64, 25),
        None,
    ),
    c(
        ID_LABEL_LOG_UNIT,
        Kind::Label,
        ADVANCED,
        (238, 525, 210, 20),
        Some(|s| s.log_limit_unit),
    ),
    c(
        ID_LOG_NOTE,
        Kind::Hint,
        ADVANCED,
        (32, 553, 416, 34),
        Some(|s| s.log_note),
    ),
    // Keys & mouse: mouse card
    c(
        ID_TOGGLE_MOUSE,
        Kind::Toggle,
        KEYS,
        (32, 128, 416, 26),
        Some(|s| s.filter_mouse),
    ),
    c(
        ID_MOUSE_MS_LABEL,
        Kind::Label,
        KEYS,
        (32, 163, 150, 20),
        Some(|s| s.mouse_threshold_label),
    ),
    c(
        ID_MOUSE_MS_EDIT,
        Kind::NumberEdit,
        KEYS,
        (190, 160, 56, 25),
        None,
    ),
    c(
        ID_MOUSE_MS_UNIT,
        Kind::Label,
        KEYS,
        (252, 163, 26, 20),
        Some(|s| s.unit_ms),
    ),
    c(
        ID_BTN_MOUSE_MS_SET,
        Kind::Button,
        KEYS,
        (282, 158, 80, 29),
        Some(|s| s.key_set_btn),
    ),
    c(
        ID_MOUSE_HINT,
        Kind::Hint,
        KEYS,
        (32, 194, 416, 46),
        Some(|s| s.mouse_hint),
    ),
    // Keys & mouse: per-key thresholds card
    c(
        ID_KEYS_TITLE,
        Kind::Section,
        KEYS,
        (32, 268, 144, 20),
        Some(|s| s.card_keys),
    ),
    c(
        ID_BTN_KEY_APPLY_ALL,
        Kind::Button,
        KEYS,
        (184, 262, 130, 29),
        Some(|s| s.key_apply_all_btn),
    ),
    c(
        ID_BTN_KEY_STATS,
        Kind::Button,
        KEYS,
        (320, 262, 128, 29),
        Some(|s| s.key_stats_btn),
    ),
    c(ID_KEYS_LIST, Kind::List, KEYS, (32, 296, 416, 184), None),
    c(ID_KEYS_COMBO, Kind::Combo, KEYS, (32, 490, 150, 240), None),
    c(
        ID_KEYS_EDIT,
        Kind::NumberEdit,
        KEYS,
        (190, 490, 56, 25),
        None,
    ),
    c(
        ID_KEYS_UNIT,
        Kind::Label,
        KEYS,
        (252, 493, 26, 20),
        Some(|s| s.unit_ms),
    ),
    c(
        ID_BTN_KEY_SET,
        Kind::Button,
        KEYS,
        (282, 488, 80, 29),
        Some(|s| s.key_set_btn),
    ),
    c(
        ID_BTN_KEY_RESET,
        Kind::Button,
        KEYS,
        (368, 488, 80, 29),
        Some(|s| s.key_reset_btn),
    ),
    c(ID_KEYS_ADVICE, Kind::Hint, KEYS, (32, 524, 416, 36), None),
    c(
        ID_KEYS_NOTE,
        Kind::Hint,
        KEYS,
        (32, 562, 416, 34),
        Some(|s| s.keys_note),
    ),
    // Save row (Settings + Advanced)
    w(
        ID_BTN_SAVE,
        Kind::Primary,
        SETTINGS | ADVANCED,
        (16, 614, 120, 34),
        Some(|s| s.save),
    ),
    w(
        ID_SAVE_STATUS,
        Kind::Status,
        SETTINGS | ADVANCED,
        (148, 622, 316, 20),
        None,
    ),
    // Footer
    w(ID_FOOTER, Kind::Footer, EVERY, (16, 664, 448, 18), None),
];

/// Rounded track behind the tab buttons.
pub(crate) const TAB_TRACK: (i32, i32, i32, i32) = (16, 68, 448, 38);

/// Vertical dividers between the three Home statistics (x, y0, y1).
pub(crate) const STAT_DIVIDERS: [(i32, i32, i32); 2] = [(165, 266, 312), (307, 266, 312)];
