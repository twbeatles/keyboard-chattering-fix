//! Settings model: every persisted option in one place.
//!
//! Serialization lives in `storage`; startup integration in `autostart`;
//! sound-path validation in `sound`.

use crate::{
    debounce::{
        Debouncer, GAP_BUCKETS, GapHistogram, MOUSE_THRESHOLD_DEFAULT_MS, MeasureFrom,
        THRESHOLD_DEFAULT_MS,
    },
    i18n::{Language, LanguagePref, system_language},
};

use windows::Win32::System::SystemInformation::GetLocalTime;

/// Keep the newest daily entries; the trend window only looks back weeks.
fn prune_days(history: &mut Vec<DayEntry>) {
    history.sort_by_key(|e| e.date);
    if history.len() > HISTORY_DAYS_KEPT {
        history.drain(..history.len() - HISTORY_DAYS_KEPT);
    }
}

/// Every key's measured gaps, zeroed.
pub fn empty_histograms() -> Box<[GapHistogram; 256]> {
    vec![[0; GAP_BUCKETS]; 256]
        .into_boxed_slice()
        .try_into()
        .expect("256 histograms")
}

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
    /// 0 means the key is never filtered.
    pub per_key_ms: [Option<u64>; 256],
    /// Block window of mouse buttons without their own (`iMouseThreshold`).
    pub mouse_threshold_ms: u64,
    pub measure_from: MeasureFrom,
    pub min_chatter_ms: u64,
    pub exclude_injected: bool,
    /// Also filter mouse button chatter (a click registering twice).
    pub filter_mouse: bool,
    /// Also filter mouse-wheel reversals (a worn encoder briefly scrolling
    /// the opposite way). Same-direction scrolling always passes.
    pub filter_wheel: bool,
    /// Check for updates automatically, about once a week (tray balloon
    /// only, never a dialog). Turn it off with `bAutoCheckUpdate=0`.
    pub auto_check_update: bool,
    /// Unix time of the last finished update check (manual or automatic),
    /// 0 when never checked. The next automatic check runs a week after it.
    pub last_update_check_unix: u64,
    /// Hold each release of a held key back this long, 0 = off.
    pub release_defer_ms: u64,
    /// Executables that pause protection while in front, as typed
    /// (`game.exe; other`); see `ui::foreground::parse_app_list`.
    pub auto_disable_apps: String,
    /// Per-app block windows, as typed (`game.exe=80; editor.exe=25`); see
    /// `ui::foreground::parse_app_profiles`. Empty = no profiles: every app
    /// uses the global window. A value of 0 never filters that app (but
    /// still counts presses), like `key.XX=0`.
    pub app_profiles: String,
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
    /// Measured gaps per key (`hist.XX`), the input of the threshold
    /// suggestion. Measured from `measure_from`; kept across restarts so
    /// rare chatter adds up instead of starting over every sign-in.
    pub gap_hist: Box<[GapHistogram; 256]>,
    /// Per-day per-key (presses, blocked) (`day.YYYYMMDD`), newest last.
    /// The trend diagnosis ("recently worse?") reads this, not the
    /// cumulative counters.
    pub day_history: Vec<DayEntry>,
    /// Keys already warned about, per day (`alert.YYYYMMDD`): one balloon
    /// per key per day, even across restarts.
    pub alerted: Vec<(u32, Vec<u8>)>,
}

/// One day of per-key totals, indexed by virtual-key code.
#[derive(Clone, Debug, PartialEq)]
pub struct DayEntry {
    /// Local calendar day as YYYYMMDD.
    pub date: u32,
    pub presses: [u64; 256],
    pub blocked: [u64; 256],
}

/// How many daily entries are kept on disk.
pub(crate) const HISTORY_DAYS_KEPT: usize = 30;

/// Local calendar day as YYYYMMDD (the day-history key).
pub fn today_yyyymmdd() -> u32 {
    let t = unsafe { GetLocalTime() };
    (u32::from(t.wYear) * 100 + u32::from(t.wMonth)) * 100 + u32::from(t.wDay)
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

    /// Take the counters and measured gaps from the live filter (they are
    /// owned by it, not by the file). Gaps measured from another baseline
    /// than `self.measure_from` are not taken: they would mean something
    /// else once read back.
    ///
    /// Day totals are consume-on-take: each event is folded into the day it
    /// was measured on exactly once, so a midnight crossing attributes its
    /// tail correctly and nothing is ever double-counted. `today` is a
    /// parameter (rather than read here) so tests can drive any date.
    pub fn take_live_stats(&mut self, d: &mut Debouncer) {
        let today = today_yyyymmdd();
        self.take_live_stats_at(d, today);
    }

    /// [`take_live_stats`] with the date given explicitly, so tests can
    /// drive any date (midnight rollover included) without waiting for one.
    pub(crate) fn take_live_stats_at(&mut self, d: &mut Debouncer, today: u32) {
        self.total_presses = d.total_presses;
        self.blocked_presses = d.blocked_presses;
        self.per_key_presses = d.per_key_presses;
        self.per_key_blocked = d.per_key_blocked;
        if d.measure_from() == self.measure_from {
            *self.gap_hist = *d.gap_histograms();
        } else {
            self.gap_hist.iter_mut().for_each(|h| *h = [0; GAP_BUCKETS]);
        }
        let target = if d.day_date == 0 { today } else { d.day_date };
        let active = d.day_presses.iter().any(|&n| n > 0) || d.day_blocked.iter().any(|&n| n > 0);
        if active {
            match self.day_history.iter_mut().find(|e| e.date == target) {
                Some(e) => {
                    for vk in 0..256 {
                        e.presses[vk] = e.presses[vk].saturating_add(d.day_presses[vk]);
                        e.blocked[vk] = e.blocked[vk].saturating_add(d.day_blocked[vk]);
                    }
                }
                None => self.day_history.push(DayEntry {
                    date: target,
                    presses: d.day_presses,
                    blocked: d.day_blocked,
                }),
            }
        }
        d.day_presses = [0; 256];
        d.day_blocked = [0; 256];
        d.day_date = today;
        prune_days(&mut self.day_history);
        let kept: Vec<u32> = self.day_history.iter().map(|e| e.date).collect();
        self.alerted.retain(|(date, _)| kept.contains(date));
    }

    /// Hand the saved counters and measured gaps to a new filter (startup).
    pub fn restore_live_stats(&self, d: &mut Debouncer) {
        d.total_presses = self.total_presses;
        d.blocked_presses = self.blocked_presses;
        d.per_key_presses = self.per_key_presses;
        d.per_key_blocked = self.per_key_blocked;
        if d.measure_from() == self.measure_from {
            d.set_gap_histograms(&self.gap_hist);
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_ms: THRESHOLD_DEFAULT_MS,
            enabled: true,
            play_sound: false,
            sound_file: None,
            minimize_on_launch: false,
            close_to_tray: true,
            language: LanguagePref::Auto,
            per_key_ms: [None; 256],
            mouse_threshold_ms: MOUSE_THRESHOLD_DEFAULT_MS,
            measure_from: MeasureFrom::Release,
            min_chatter_ms: 0,
            exclude_injected: true,
            filter_mouse: false,
            filter_wheel: false,
            auto_check_update: true,
            last_update_check_unix: 0,
            release_defer_ms: 0,
            auto_disable_apps: String::new(),
            app_profiles: String::new(),
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
            gap_hist: empty_histograms(),
            day_history: Vec::new(),
            alerted: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debounce::KeyEventKind;
    use std::time::Duration;

    /// A filter that measured one 6 ms bounce of A (bucket 1).
    fn measured(measure_from: MeasureFrom) -> Debouncer {
        let mut d = Debouncer::new(50);
        d.set_measure_from(measure_from);
        let ms = Duration::from_millis;
        d.decide_event(0x41, false, KeyEventKind::Down, ms(0));
        d.decide_event(0x41, false, KeyEventKind::Up, ms(100));
        d.decide_event(0x41, false, KeyEventKind::Down, ms(106));
        d
    }

    #[test]
    fn live_stats_round_trip_through_the_settings() {
        let mut d = measured(MeasureFrom::Release);
        let mut s = Settings::default();
        s.take_live_stats(&mut d);
        assert_eq!((s.total_presses, s.blocked_presses), (2, 1));
        assert_eq!(s.gap_hist[0x41][1], 1);

        let mut restarted = Debouncer::new(50);
        s.restore_live_stats(&mut restarted);
        assert_eq!(restarted.per_key_blocked[0x41], 1);
        assert_eq!(restarted.gap_histogram(0x41), d.gap_histogram(0x41));
    }

    /// Gaps never cross baselines, in either direction: counters do.
    /// Day totals consume each event exactly once: a second take without
    /// new input adds nothing, and a midnight crossing attributes its tail
    /// to the day it was measured on.
    #[test]
    fn day_history_consumes_each_event_once() {
        use crate::debounce::KeyEventKind;
        let ms = Duration::from_millis;
        let mut d = Debouncer::new(50);
        let mut s = Settings::default();
        d.decide_event(0x41, false, KeyEventKind::Down, ms(0));
        d.decide_event(0x41, false, KeyEventKind::Up, ms(10));
        s.take_live_stats_at(&mut d, 20261124);
        // A second take with no new input changes nothing.
        s.take_live_stats_at(&mut d, 20261124);
        assert_eq!(s.day_history.len(), 1);
        assert_eq!(s.day_history[0].date, 20261124);
        assert_eq!(s.day_history[0].presses[0x41], 1);
        // A take after midnight with no new input adds no empty day.
        s.take_live_stats_at(&mut d, 20261125);
        assert_eq!(s.day_history.len(), 1);
        // Input on the new day lands on the new day.
        d.decide_event(0x41, false, KeyEventKind::Down, ms(20));
        d.decide_event(0x41, false, KeyEventKind::Up, ms(30));
        s.take_live_stats_at(&mut d, 20261125);
        assert_eq!(s.day_history.len(), 2);
        let old = s.day_history.iter().find(|e| e.date == 20261124).unwrap();
        let new = s.day_history.iter().find(|e| e.date == 20261125).unwrap();
        assert_eq!(old.presses[0x41], 1);
        assert_eq!(new.presses[0x41], 1);
    }

    /// Only the newest 30 days are kept; the trend window looks back weeks.
    #[test]
    fn day_history_prunes_to_thirty_days() {
        let mut s = Settings::default();
        for day in 1..=35u32 {
            s.day_history.push(DayEntry {
                date: 20261100 + day,
                presses: [0; 256],
                blocked: [0; 256],
            });
        }
        let mut d = Debouncer::new(50);
        s.take_live_stats_at(&mut d, 20261135);
        assert_eq!(s.day_history.len(), 30);
        assert_eq!(s.day_history[0].date, 20261106);
        assert_eq!(s.day_history[29].date, 20261135);
    }

    #[test]
    fn gaps_from_another_baseline_are_neither_saved_nor_restored() {
        let mut d = measured(MeasureFrom::Press);
        let mut s = Settings::default();
        s.gap_hist[0x42][3] = 5;
        s.take_live_stats(&mut d);
        assert_eq!(s.total_presses, 2);
        assert_eq!(s.gap_hist.iter().flatten().sum::<u32>(), 0);

        s.gap_hist[0x42][3] = 5;
        let mut press = Debouncer::new(50);
        press.set_measure_from(MeasureFrom::Press);
        s.restore_live_stats(&mut press);
        assert_eq!(press.per_key_presses[0x41], 2);
        assert_eq!(press.gap_histograms().iter().flatten().sum::<u32>(), 0);
    }
}
