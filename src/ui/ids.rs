//! Window control IDs, menu command IDs, and private window messages.
//!
//! The numeric layout mirrors the window: 100s chrome (header, tabs,
//! footer), 200s Home, 250s cleaning overlay, 300s History, 400s Settings,
//! 500s Advanced, 600s the shared save row, 700s Keys & mouse, 1000s menu
//! commands.

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

// Chrome.
pub(crate) const ID_HEADER_TITLE: i32 = 100;
pub(crate) const ID_HEADER_SUB: i32 = 101;
pub(crate) const ID_PILL: i32 = 102;
pub(crate) const ID_TAB_HOME: i32 = 110;
pub(crate) const ID_TAB_HISTORY: i32 = 111;
pub(crate) const ID_TAB_SETTINGS: i32 = 112;
pub(crate) const ID_TAB_ADVANCED: i32 = 113;
pub(crate) const ID_TAB_KEYS: i32 = 114;
pub(crate) const ID_FOOTER: i32 = 120;

// Home.
pub(crate) const ID_TOGGLE_ENABLE: i32 = 200;
pub(crate) const ID_HOME_DESC: i32 = 201;
pub(crate) const ID_STATS_TITLE: i32 = 202;
pub(crate) const ID_STAT_TOTAL_LABEL: i32 = 203;
pub(crate) const ID_STAT_TOTAL_VAL: i32 = 204;
pub(crate) const ID_STAT_BLOCKED_LABEL: i32 = 205;
pub(crate) const ID_STAT_BLOCKED_VAL: i32 = 206;
pub(crate) const ID_STAT_RATIO_LABEL: i32 = 207;
pub(crate) const ID_STAT_RATIO_VAL: i32 = 208;
pub(crate) const ID_STAT_LAST: i32 = 209;
pub(crate) const ID_STAT_TOP: i32 = 210;
pub(crate) const ID_TOOLS_TITLE: i32 = 211;
pub(crate) const ID_BTN_CLEAN_MODE: i32 = 212;
pub(crate) const ID_BTN_RESET_STATS: i32 = 213;
pub(crate) const ID_BTN_OPEN_CPL: i32 = 214;
pub(crate) const ID_HOME_TIP: i32 = 215;
/// Live notes: paused for an app, elevated window in front, hook restarted.
pub(crate) const ID_HOME_NOTE: i32 = 216;

// Cleaning-mode overlay.
pub(crate) const ID_CLEAN_TITLE: i32 = 250;
pub(crate) const ID_CLEAN_BODY: i32 = 251;
pub(crate) const ID_BTN_CLEAN_END: i32 = 252;

// History.
pub(crate) const ID_HIST_VIEW: i32 = 300;
pub(crate) const ID_HIST_SEARCH: i32 = 301;
pub(crate) const ID_BTN_CLEAR_LOG: i32 = 302;
pub(crate) const ID_HIST_LIST: i32 = 303;
pub(crate) const ID_HIST_INFO: i32 = 304;

// Settings.
pub(crate) const ID_THRESH_TITLE: i32 = 400;
pub(crate) const ID_LABEL_THRESHOLD: i32 = 401;
pub(crate) const ID_EDIT_THRESHOLD: i32 = 402;
pub(crate) const ID_LABEL_UNIT: i32 = 403;
pub(crate) const ID_THRESH_HINT: i32 = 404;
pub(crate) const ID_SOUND_TITLE: i32 = 405;
pub(crate) const ID_TOGGLE_SOUND: i32 = 406;
pub(crate) const ID_BTN_SOUND_TEST: i32 = 407;
pub(crate) const ID_EDIT_SOUND_FILE: i32 = 408;
pub(crate) const ID_BTN_SOUND_BROWSE: i32 = 409;
pub(crate) const ID_APP_TITLE: i32 = 410;
pub(crate) const ID_TOGGLE_AUTOSTART: i32 = 411;
pub(crate) const ID_TOGGLE_MINIMIZE: i32 = 412;
pub(crate) const ID_TOGGLE_CLOSE_TO_TRAY: i32 = 413;
pub(crate) const ID_LABEL_LANG: i32 = 414;
pub(crate) const ID_COMBO_LANG: i32 = 415;
pub(crate) const ID_LABEL_AUTO_APPS: i32 = 416;
pub(crate) const ID_EDIT_AUTO_APPS: i32 = 417;
pub(crate) const ID_TOGGLE_AUTO_FULLSCREEN: i32 = 418;

// Advanced.
pub(crate) const ID_DETECT_TITLE: i32 = 500;
pub(crate) const ID_LABEL_MEASURE: i32 = 501;
pub(crate) const ID_COMBO_MEASURE: i32 = 502;
pub(crate) const ID_LABEL_MIN_CHATTER: i32 = 503;
pub(crate) const ID_EDIT_MIN_CHATTER: i32 = 504;
pub(crate) const ID_LABEL_MIN_UNIT: i32 = 505;
pub(crate) const ID_MIN_HINT: i32 = 506;
pub(crate) const ID_TOGGLE_EXCLUDE_INJECTED: i32 = 507;
pub(crate) const ID_LOG_TITLE: i32 = 508;
pub(crate) const ID_TOGGLE_RECORD_ALL: i32 = 509;
pub(crate) const ID_LABEL_LOG_LIMIT: i32 = 510;
pub(crate) const ID_EDIT_LOG_LIMIT: i32 = 511;
pub(crate) const ID_LABEL_LOG_UNIT: i32 = 512;
pub(crate) const ID_LOG_NOTE: i32 = 513;
pub(crate) const ID_LABEL_DEFER: i32 = 514;
pub(crate) const ID_EDIT_DEFER: i32 = 515;
pub(crate) const ID_LABEL_DEFER_UNIT: i32 = 516;
pub(crate) const ID_DEFER_HINT: i32 = 517;
pub(crate) const ID_LABEL_HOTKEYS: i32 = 518;
pub(crate) const ID_EDIT_HOTKEY_TOGGLE: i32 = 519;
pub(crate) const ID_EDIT_HOTKEY_CLEAN: i32 = 520;

// Save row (Settings + Advanced).
pub(crate) const ID_BTN_SAVE: i32 = 600;
pub(crate) const ID_SAVE_STATUS: i32 = 601;

// Keys & mouse.
pub(crate) const ID_TOGGLE_MOUSE: i32 = 700;
pub(crate) const ID_MOUSE_HINT: i32 = 701;
pub(crate) const ID_KEYS_TITLE: i32 = 702;
pub(crate) const ID_KEYS_LIST: i32 = 703;
pub(crate) const ID_KEYS_COMBO: i32 = 704;
pub(crate) const ID_KEYS_EDIT: i32 = 705;
pub(crate) const ID_KEYS_UNIT: i32 = 706;
pub(crate) const ID_BTN_KEY_SET: i32 = 707;
pub(crate) const ID_BTN_KEY_RESET: i32 = 708;
pub(crate) const ID_KEYS_ADVICE: i32 = 709;
pub(crate) const ID_KEYS_NOTE: i32 = 710;

// Menu commands (menu bar and tray share them).
pub(crate) const MENU_SHOW: usize = 1001;
pub(crate) const MENU_TOGGLE: usize = 1002;
pub(crate) const MENU_CLEANING: usize = 1003;
pub(crate) const MENU_EXIT: usize = 1004;
pub(crate) const MENU_UPDATE: usize = 1005;
pub(crate) const MENU_HIDE: usize = 1006;
pub(crate) const MENU_RESET_STATS: usize = 1007;
pub(crate) const MENU_CLEAR_LOG: usize = 1008;
pub(crate) const MENU_OPEN_CPL: usize = 1009;
pub(crate) const MENU_OPEN_CONFIG: usize = 1010;
pub(crate) const MENU_HOMEPAGE: usize = 1011;
pub(crate) const MENU_ABOUT: usize = 1012;
pub(crate) const MENU_EXPORT_HISTORY: usize = 1013;
pub(crate) const MENU_EXPORT_SETTINGS: usize = 1014;
pub(crate) const MENU_IMPORT_SETTINGS: usize = 1015;
pub(crate) const MENU_RUN_ELEVATED: usize = 1016;
pub(crate) const MENU_ELEVATED_AUTOSTART: usize = 1017;

pub(crate) const WM_TRAY: u32 = WM_APP + 1;
/// Posted from the update worker thread after the helper is launched; the
/// window must be destroyed on the UI thread (this process has to exit so
/// the helper can replace the executable file).
pub(crate) const WM_UPDATE_EXIT: u32 = WM_APP + 2;
pub(crate) const TRAY_ID: u32 = 1;
/// `RegisterHotKey` ids.
pub(crate) const HOTKEY_TOGGLE: i32 = 1;
pub(crate) const HOTKEY_CLEANING: i32 = 2;
/// The 500 ms refresh timer.
pub(crate) const TIMER_STATS: usize = 1;
