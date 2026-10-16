//! Window control IDs, tray menu IDs, and private window messages.
//!
//! The numeric layout mirrors the card order on the window (100s header,
//! 200s telemetry, 300s settings, 400s options, 600s actions).

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

// Control IDs.
pub(crate) const ID_HEADER_TITLE: i32 = 100;
pub(crate) const ID_HEADER_SUB: i32 = 101;
pub(crate) const ID_HEADER_BADGE: i32 = 102;
pub(crate) const ID_CLEAN_BANNER: i32 = 103;

pub(crate) const ID_CARD1_TITLE: i32 = 200;
pub(crate) const ID_STAT_TOTAL_LABEL: i32 = 201;
pub(crate) const ID_STAT_TOTAL_VAL: i32 = 202;
pub(crate) const ID_STAT_CHATTER_LABEL: i32 = 203;
pub(crate) const ID_STAT_CHATTER_VAL: i32 = 204;
pub(crate) const ID_STAT_RATIO_LABEL: i32 = 205;
pub(crate) const ID_STAT_RATIO_VAL: i32 = 206;
pub(crate) const ID_STATIC_LAST: i32 = 207;
pub(crate) const ID_STATIC_TOP_CHATTER: i32 = 208;

pub(crate) const ID_CARD2_TITLE: i32 = 300;
pub(crate) const ID_LABEL_THRESHOLD: i32 = 301;
pub(crate) const ID_EDIT_THRESHOLD: i32 = 302;
pub(crate) const ID_LABEL_UNIT: i32 = 303;
pub(crate) const ID_CHECK_ENABLE: i32 = 304;
pub(crate) const ID_CHECK_SOUND: i32 = 305;
pub(crate) const ID_LABEL_SOUND_FILE: i32 = 306;
pub(crate) const ID_EDIT_SOUND_FILE: i32 = 307;
pub(crate) const ID_BTN_SOUND_TEST: i32 = 308;
pub(crate) const ID_CHECK_AUTOSTART: i32 = 309;
pub(crate) const ID_CHECK_MINIMIZE: i32 = 310;
pub(crate) const ID_CHECK_CLOSE_TO_TRAY: i32 = 311;
pub(crate) const ID_CHECK_RECORD_ALL: i32 = 312;

pub(crate) const ID_CARD3_TITLE: i32 = 400;
pub(crate) const ID_BTN_CLEAN_MODE: i32 = 401;
pub(crate) const ID_BTN_OPEN_CPL: i32 = 402;

pub(crate) const ID_CARD4_TITLE: i32 = 500;
pub(crate) const ID_LABEL_MEASURE: i32 = 501;
pub(crate) const ID_COMBO_MEASURE: i32 = 502;
pub(crate) const ID_LABEL_LANG: i32 = 503;
pub(crate) const ID_COMBO_LANG: i32 = 504;
pub(crate) const ID_LABEL_MIN_CHATTER: i32 = 505;
pub(crate) const ID_EDIT_MIN_CHATTER: i32 = 506;
pub(crate) const ID_LABEL_MIN_UNIT: i32 = 507;
pub(crate) const ID_CHECK_EXCLUDE_INJECTED: i32 = 508;
pub(crate) const ID_LABEL_LOG_LIMIT: i32 = 509;
pub(crate) const ID_EDIT_LOG_LIMIT: i32 = 510;
pub(crate) const ID_LABEL_LOG_UNIT: i32 = 511;

pub(crate) const ID_BTN_SAVE: i32 = 601;
pub(crate) const ID_STATIC_HINT: i32 = 602;

// Tray menu IDs.
pub(crate) const MENU_SHOW: usize = 1001;
pub(crate) const MENU_TOGGLE: usize = 1002;
pub(crate) const MENU_CLEANING: usize = 1003;
pub(crate) const MENU_EXIT: usize = 1004;
pub(crate) const MENU_UPDATE: usize = 1005;

pub(crate) const WM_TRAY: u32 = WM_APP + 1;
/// Posted from the update worker thread after the helper is launched; the
/// window must be destroyed on the UI thread (this process has to exit so
/// the helper can replace the executable file).
pub(crate) const WM_UPDATE_EXIT: u32 = WM_APP + 2;
pub(crate) const TRAY_ID: u32 = 1;
