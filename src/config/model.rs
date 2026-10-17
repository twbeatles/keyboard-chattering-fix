//! Settings model: every persisted option in one place.
//!
//! Serialization lives in `storage`; startup integration in `autostart`;
//! sound-path validation in `sound`.

use crate::{
    debounce::MeasureFrom,
    i18n::{system_language, Language, LanguagePref},
};

#[derive(Clone, Debug)]
pub struct Settings {
    pub threshold_ms: u64,
    pub enabled: bool,
    pub play_sound: bool,
    pub sound_file: Option<String>,
    pub minimize_on_launch: bool,
    pub close_to_tray: bool,
    /// The picker choice: follow Windows (default) or a fixed language.
    pub language: LanguagePref,
    /// Per-key threshold overrides in ms, indexed by virtual-key code.
    pub per_key_ms: [Option<u64>; 256],
    pub measure_from: MeasureFrom,
    pub min_chatter_ms: u64,
    pub exclude_injected: bool,
    /// Also filter mouse button chatter (a click registering twice).
    pub filter_mouse: bool,
    /// Hold each release of a held key back this long, 0 = off.
    pub release_defer_ms: u64,
    /// Executables that pause protection while in front, as typed
    /// (`game.exe; other`); see `ui::foreground::parse_app_list`.
    pub auto_disable_apps: String,
    /// Pause protection while a full-screen app is in front.
    pub auto_disable_fullscreen: bool,
    /// Global hotkeys as typed (`Ctrl+Alt+F12`); empty = none.
    pub hotkey_toggle: String,
    pub hotkey_cleaning: String,
    /// Cleaning mode ends by itself after this many minutes, 0 = never.
    pub cleaning_timeout_min: u64,
    pub record_all_keys: bool,
    pub log_limit: usize,
    pub total_presses: u64,
    pub blocked_presses: u64,
    /// Per-key cumulative counters, indexed by virtual-key code. Persisted
    /// alongside the totals so the top-chatter diagnosis survives restarts.
    pub per_key_presses: [u64; 256],
    pub per_key_blocked: [u64; 256],
}

impl Settings {
    /// The language to display now: the fixed choice, or Windows' own.
    pub fn ui_language(&self) -> Language {
        self.language.resolve(system_language())
    }

    /// Take the file-owned fields from `disk`, the settings file as it is
    /// now. Per-key thresholds (`key.XX`) can be edited in the file while
    /// the app runs; the Keys tab writes its changes to the file at once,
    /// so the file is always their source of truth. Writing the in-memory
    /// copy without this would silently revert file edits on exit
    /// (ISSUE-005). Every other field is owned by the UI or the live filter
    /// and keeps its value.
    pub fn adopt_file_only_fields(&mut self, disk: &Settings) {
        self.per_key_ms = disk.per_key_ms;
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_ms: 50,
            enabled: true,
            play_sound: false,
            sound_file: None,
            minimize_on_launch: false,
            close_to_tray: true,
            language: LanguagePref::Auto,
            per_key_ms: [None; 256],
            measure_from: MeasureFrom::Release,
            min_chatter_ms: 0,
            exclude_injected: true,
            filter_mouse: false,
            release_defer_ms: 0,
            auto_disable_apps: String::new(),
            auto_disable_fullscreen: false,
            hotkey_toggle: String::new(),
            hotkey_cleaning: String::new(),
            cleaning_timeout_min: 5,
            record_all_keys: false,
            log_limit: 100,
            total_presses: 0,
            blocked_presses: 0,
            per_key_presses: [0; 256],
            per_key_blocked: [0; 256],
        }
    }
}
