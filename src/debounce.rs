use std::{collections::VecDeque, time::Duration};

/// Press/release event for one virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyEventKind {
    Down,
    Up,
}

/// Reference point for measuring the chatter gap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeasureFrom {
    /// Gap since the last *accepted press* (like the original kcfix).
    Press,
    /// Gap since the last *release* (default; immune to slow releases).
    Release,
}

/// A recorded key event, shown in the window's History tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogEvent {
    pub time: Duration,
    pub vk: u8,
    pub blocked: bool,
    pub gap_ms: Option<u64>,
}

/// Time-based chatter filter.
///
/// A `Down` is blocked when the same key fires again inside its threshold.
/// Auto-repeat (a `Down` with no `Up` in between) is always forwarded, so
/// holding a key never loses characters.
pub struct Debouncer {
    global_threshold: Duration,
    per_key: [Option<Duration>; 256],
    measure_from: MeasureFrom,
    min_chatter: Duration,
    last_release: [Option<Duration>; 256],
    last_press: [Option<Duration>; 256],
    held: [bool; 256],
    pub total_presses: u64,
    pub blocked_presses: u64,
    pub per_key_presses: [u64; 256],
    pub per_key_blocked: [u64; 256],
    /// (virtual-key code, observed gap) of the most recent block.
    pub last_blocked: Option<(u8, Duration)>,
    /// Keyboard cleaning mode: when true, all keystrokes are blocked until Enter (VK_RETURN) is pressed.
    pub cleaning_mode: bool,
    /// Real-time event log buffer. Capacity is reserved up front by
    /// [`Debouncer::set_log_limit`], so the hook never allocates here.
    pub log_events: VecDeque<LogEvent>,
    /// Record all keypresses or only blocked chatter events.
    pub record_all_keys: bool,
    /// Maximum log capacity; change it only through `set_log_limit`.
    log_limit: usize,
    /// Bumped on every log or counter reset change, so the History tab can
    /// skip re-rendering when nothing new was recorded.
    pub log_seq: u64,
}

/// Bounds of the in-memory event log (UI and settings file share them).
pub const LOG_LIMIT_MIN: usize = 10;
pub const LOG_LIMIT_MAX: usize = 5000;

impl Debouncer {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            global_threshold: Duration::from_millis(threshold_ms.clamp(1, 1000)),
            per_key: [None; 256],
            measure_from: MeasureFrom::Release,
            min_chatter: Duration::ZERO,
            last_release: [None; 256],
            last_press: [None; 256],
            held: [false; 256],
            total_presses: 0,
            blocked_presses: 0,
            per_key_presses: [0; 256],
            per_key_blocked: [0; 256],
            last_blocked: None,
            cleaning_mode: false,
            log_events: VecDeque::with_capacity(100),
            record_all_keys: false,
            log_limit: 100,
            log_seq: 0,
        }
    }

    pub fn log_limit(&self) -> usize {
        self.log_limit
    }

    /// Resize the event log. Called on the UI thread (startup/save): the
    /// buffer is trimmed and its full capacity reserved here, so `push_log`
    /// on the hook thread never grows the allocation.
    pub fn set_log_limit(&mut self, limit: usize) {
        self.log_limit = limit.clamp(LOG_LIMIT_MIN, LOG_LIMIT_MAX);
        while self.log_events.len() > self.log_limit {
            self.log_events.pop_front();
        }
        let extra = self.log_limit.saturating_sub(self.log_events.len());
        self.log_events.reserve_exact(extra);
        self.log_seq += 1;
    }

    /// Drop every recorded event (counters are kept).
    pub fn clear_log(&mut self) {
        self.log_events.clear();
        self.log_seq += 1;
    }

    /// Zero every counter and the event log ("Reset statistics").
    pub fn reset_stats(&mut self) {
        self.total_presses = 0;
        self.blocked_presses = 0;
        self.per_key_presses = [0; 256];
        self.per_key_blocked = [0; 256];
        self.last_blocked = None;
        self.clear_log();
    }

    /// Effective threshold for `vk` in ms and whether it is a per-key
    /// override (`key.XX` in the settings file) rather than the global one.
    pub fn key_threshold_ms(&self, vk: u8) -> (u64, bool) {
        match self.per_key[vk as usize] {
            Some(d) => (d.as_millis() as u64, true),
            None => (self.threshold_ms(), false),
        }
    }

    pub fn set_threshold_ms(&mut self, ms: u64) {
        self.global_threshold = Duration::from_millis(ms.clamp(1, 1000));
    }

    pub fn threshold_ms(&self) -> u64 {
        self.global_threshold.as_millis() as u64
    }

    pub fn min_chatter_ms(&self) -> u64 {
        self.min_chatter.as_millis() as u64
    }

    /// Per-key override (`None` removes it, falling back to global).
    /// `None` as `ms` clears the override.
    pub fn set_key_threshold(&mut self, vk: u8, ms: Option<u64>) {
        self.per_key[vk as usize] = ms.map(|m| Duration::from_millis(m.clamp(1, 1000)));
    }

    pub fn set_measure_from(&mut self, m: MeasureFrom) {
        self.measure_from = m;
    }

    /// Gaps shorter than this are treated as bugged 0 ms-class input and
    /// forwarded, never counted as chatter.
    pub fn set_min_chatter_ms(&mut self, ms: u64) {
        self.min_chatter = Duration::from_millis(ms.min(1000));
    }

    fn effective_threshold(&self, vk: usize) -> Duration {
        self.per_key[vk].unwrap_or(self.global_threshold)
    }

    fn push_log(&mut self, event: LogEvent) {
        if self.log_events.len() >= self.log_limit {
            self.log_events.pop_front();
        }
        self.log_events.push_back(event);
        self.log_seq += 1;
    }

    /// Returns `true` when the event should reach applications.
    pub fn decide(&mut self, vk: usize, kind: KeyEventKind, now: Duration) -> bool {
        let vk = vk & 0xFF;

        // Keyboard cleaning mode: block all keystrokes. Enter (0x0D) exits cleaning mode.
        if self.cleaning_mode {
            if kind == KeyEventKind::Down && vk == 0x0D {
                self.cleaning_mode = false;
            }
            return false;
        }

        match kind {
            KeyEventKind::Up => {
                self.held[vk] = false;
                self.last_release[vk] = Some(now);
                true
            }
            KeyEventKind::Down => {
                self.total_presses += 1;
                self.per_key_presses[vk] += 1;

                // No release in between: genuine auto-repeat, never chatter.
                if self.held[vk] {
                    self.last_press[vk] = Some(now);
                    if self.record_all_keys {
                        self.push_log(LogEvent {
                            time: now,
                            vk: vk as u8,
                            blocked: false,
                            gap_ms: None,
                        });
                    }
                    return true;
                }

                let anchor = match self.measure_from {
                    MeasureFrom::Release => self.last_release[vk],
                    MeasureFrom::Press => self.last_press[vk],
                };

                if let Some(prev) = anchor {
                    let gap = now.saturating_sub(prev);
                    if gap < self.min_chatter {
                        // Bugged input (e.g. 0 ms duplicate): not chatter.
                        self.held[vk] = true;
                        self.last_press[vk] = Some(now);
                        if self.record_all_keys {
                            self.push_log(LogEvent {
                                time: now,
                                vk: vk as u8,
                                blocked: false,
                                gap_ms: Some(gap.as_millis() as u64),
                            });
                        }
                        return true;
                    }
                    if gap < self.effective_threshold(vk) {
                        self.blocked_presses += 1;
                        self.per_key_blocked[vk] += 1;
                        self.last_blocked = Some((vk as u8, gap));
                        self.push_log(LogEvent {
                            time: now,
                            vk: vk as u8,
                            blocked: true,
                            gap_ms: Some(gap.as_millis() as u64),
                        });
                        return false;
                    }
                }

                self.held[vk] = true;
                self.last_press[vk] = Some(now);
                if self.record_all_keys {
                    self.push_log(LogEvent {
                        time: now,
                        vk: vk as u8,
                        blocked: false,
                        gap_ms: None,
                    });
                }
                true
            }
        }
    }

    /// Finds the key with the highest number of blocked chatter events.
    pub fn top_chattered_key(&self) -> Option<(u8, u64, f64)> {
        let mut best_vk = None;
        let mut max_blocked = 0;
        for vk in 0..256 {
            let blocked = self.per_key_blocked[vk];
            if blocked > max_blocked {
                max_blocked = blocked;
                best_vk = Some(vk as u8);
            }
        }
        best_vk.map(|vk| {
            let total = self.per_key_presses[vk as usize];
            let ratio = if total > 0 {
                max_blocked as f64 * 100.0 / total as f64
            } else {
                0.0
            };
            (vk, max_blocked, ratio)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn first_press_always_forwards() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert_eq!(d.total_presses, 1);
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn fast_bounce_is_blocked() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 20 ms after release -> inside 50 ms threshold -> blocked.
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(40)));
        assert_eq!(d.total_presses, 2);
        assert_eq!(d.blocked_presses, 1);
        assert_eq!(d.per_key_blocked[0x41], 1);
    }

    #[test]
    fn slow_repeat_is_forwarded() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 60 ms after release -> beyond 50 ms threshold -> allowed.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(80)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn auto_repeat_never_blocked() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        // Consecutive Down without Up is auto-repeat.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(30)));
        assert!(d.decide(0x41, KeyEventKind::Down, ms(50)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn keys_are_independent() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // Different key pressed 5 ms later -> allowed.
        assert!(d.decide(0x42, KeyEventKind::Down, ms(25)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn per_key_threshold_overrides_global() {
        let mut d = Debouncer::new(50);
        d.set_key_threshold(0x41, Some(100));
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 70 ms gap: would pass 50 ms global, but blocked by 100 ms override.
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(90)));
        assert_eq!(d.blocked_presses, 1);
    }

    #[test]
    fn per_key_override_can_be_cleared() {
        let mut d = Debouncer::new(50);
        d.set_key_threshold(0x41, Some(100));
        d.set_key_threshold(0x41, None);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 70 ms gap: falls back to 50 ms global, so forwards.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(90)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn measure_from_press_uses_last_press() {
        let mut d = Debouncer::new(50);
        d.set_measure_from(MeasureFrom::Press);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(55)));
        // Gap from press is 50 ms (at threshold -> forwards).
        assert!(d.decide(0x41, KeyEventKind::Down, ms(60)));
        assert_eq!(d.blocked_presses, 0);
        assert!(d.decide(0x41, KeyEventKind::Up, ms(65)));
        // Gap from last accepted press (60) is 20 ms -> blocked.
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(80)));
        assert_eq!(d.blocked_presses, 1);
    }

    #[test]
    fn boundary_at_threshold_forwards() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // Exactly 50 ms after release -> forwards.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(70)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn last_blocked_records_gap() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(35)));
        assert_eq!(d.last_blocked, Some((0x41, ms(15))));
    }

    #[test]
    fn min_chatter_forwards_bugged_input() {
        let mut d = Debouncer::new(50);
        d.set_min_chatter_ms(5);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 2 ms gap is below min_chatter -> treated as bugged duplicate, forwarded.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(22)));
        assert_eq!(d.blocked_presses, 0);
    }

    #[test]
    fn min_chatter_getter_round_trips() {
        let mut d = Debouncer::new(50);
        assert_eq!(d.min_chatter_ms(), 0);
        d.set_min_chatter_ms(2000);
        assert_eq!(d.min_chatter_ms(), 1000);
    }

    #[test]
    fn log_buffer_stays_bounded() {
        let mut d = Debouncer::new(50);
        d.record_all_keys = true;
        d.set_log_limit(10);
        for t in 0..50 {
            assert!(d.decide(0x41, KeyEventKind::Down, ms(t * 100)));
            assert!(d.decide(0x41, KeyEventKind::Up, ms(t * 100 + 50)));
        }
        // 50 recorded presses against a cap of 10: the buffer must hold
        // exactly the 10 most recent events, no more and no fewer.
        assert_eq!(d.log_events.len(), 10);
        assert_eq!(d.log_events.back().unwrap().time, ms(4900));
    }

    #[test]
    fn cleaning_mode_blocks_until_enter() {
        let mut d = Debouncer::new(50);
        d.cleaning_mode = true;
        // Normal keys blocked
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(!d.decide(0x20, KeyEventKind::Down, ms(20)));
        assert!(d.cleaning_mode);
        // Enter key (0x0D) exits cleaning mode
        assert!(!d.decide(0x0D, KeyEventKind::Down, ms(30)));
        assert!(!d.cleaning_mode);
        // After exiting, keys work normally
        assert!(d.decide(0x41, KeyEventKind::Down, ms(40)));
    }

    #[test]
    fn min_chatter_boundary_is_exclusive() {
        // gap == min_chatter is NOT treated as a glitch: it falls through to
        // the threshold check. Only strictly smaller gaps bypass filtering.
        let mut d = Debouncer::new(50);
        d.set_min_chatter_ms(10);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 9 ms < 10 ms min_chatter: forwarded as bugged input, not chatter.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(29)));
        assert_eq!(d.blocked_presses, 0);
        assert!(d.decide(0x41, KeyEventKind::Up, ms(30)));
        // Exactly 10 ms: not a glitch, and inside the 50 ms threshold: blocked.
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(40)));
        assert_eq!(d.blocked_presses, 1);
    }

    #[test]
    fn log_limit_is_clamped_and_capacity_reserved() {
        let mut d = Debouncer::new(50);
        d.set_log_limit(3);
        assert_eq!(d.log_limit(), LOG_LIMIT_MIN);
        d.set_log_limit(1_000_000);
        assert_eq!(d.log_limit(), LOG_LIMIT_MAX);
        // The whole buffer is reserved up front: the hook never reallocates.
        assert!(d.log_events.capacity() >= LOG_LIMIT_MAX);
    }

    #[test]
    fn shrinking_log_limit_keeps_newest_events() {
        let mut d = Debouncer::new(50);
        d.record_all_keys = true;
        for t in 0..20 {
            d.decide(0x41, KeyEventKind::Down, ms(t * 100));
            d.decide(0x41, KeyEventKind::Up, ms(t * 100 + 50));
        }
        d.set_log_limit(10);
        assert_eq!(d.log_events.len(), 10);
        assert_eq!(d.log_events.front().unwrap().time, ms(1000));
    }

    #[test]
    fn log_seq_moves_on_every_log_change() {
        let mut d = Debouncer::new(50);
        let s0 = d.log_seq;
        d.decide(0x41, KeyEventKind::Down, ms(10));
        // record_all_keys off and nothing blocked: no log entry, no bump.
        assert_eq!(d.log_seq, s0);
        d.decide(0x41, KeyEventKind::Up, ms(20));
        d.decide(0x41, KeyEventKind::Down, ms(30)); // blocked -> logged
        assert_eq!(d.log_seq, s0 + 1);
        d.clear_log();
        assert_eq!(d.log_seq, s0 + 2);
        assert!(d.log_events.is_empty());
    }

    #[test]
    fn reset_stats_zeroes_counters_and_log() {
        let mut d = Debouncer::new(50);
        d.decide(0x41, KeyEventKind::Down, ms(10));
        d.decide(0x41, KeyEventKind::Up, ms(20));
        d.decide(0x41, KeyEventKind::Down, ms(30));
        assert_eq!(d.blocked_presses, 1);
        d.reset_stats();
        assert_eq!((d.total_presses, d.blocked_presses), (0, 0));
        assert_eq!(d.per_key_blocked[0x41], 0);
        assert_eq!(d.last_blocked, None);
        assert!(d.log_events.is_empty());
        assert_eq!(d.top_chattered_key(), None);
    }

    #[test]
    fn key_threshold_reports_override_flag() {
        let mut d = Debouncer::new(50);
        assert_eq!(d.key_threshold_ms(0x41), (50, false));
        d.set_key_threshold(0x41, Some(120));
        assert_eq!(d.key_threshold_ms(0x41), (120, true));
        assert_eq!(d.key_threshold_ms(0x42), (50, false));
    }
}
