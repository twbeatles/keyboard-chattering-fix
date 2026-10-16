//! Settings model: every persisted option in one place.
//!
//! Serialization lives in `storage`; startup integration in `autostart`;
//! sound-path validation in `sound`.

use crate::{debounce::MeasureFrom, i18n::Language};

#[derive(Clone, Debug)]
pub struct Settings {
    pub threshold_ms: u64,
    pub enabled: bool,
    pub play_sound: bool,
    pub sound_file: Option<String>,
    pub minimize_on_launch: bool,
    pub close_to_tray: bool,
    pub language: Language,
    /// Per-key threshold overrides in ms, indexed by virtual-key code.
    pub per_key_ms: [Option<u64>; 256],
    pub measure_from: MeasureFrom,
    pub min_chatter_ms: u64,
    pub exclude_injected: bool,
    pub record_all_keys: bool,
    pub log_limit: usize,
    pub total_presses: u64,
    pub blocked_presses: u64,
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
            language: Language::Ko,
            per_key_ms: [None; 256],
            measure_from: MeasureFrom::Release,
            min_chatter_ms: 0,
            exclude_injected: true,
            record_all_keys: false,
            log_limit: 100,
            total_presses: 0,
            blocked_presses: 0,
        }
    }
}
