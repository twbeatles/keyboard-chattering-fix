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

/// `VK_PACKET`: Unicode text from IMEs, on-screen keyboards and remote
/// tools. Every character shares this code, so two different characters
/// typed quickly would look like one key chattering.
pub const VK_PACKET: usize = 0xE7;
/// `VK_PROCESSKEY`: a key the IME consumed; distinct physical keys share it.
pub const VK_PROCESSKEY: usize = 0xE5;

/// Width of one gap histogram bucket.
pub const GAP_BUCKET_MS: u64 = 5;
/// 40 buckets of 5 ms (0–199 ms) plus one for every gap of 200 ms or more.
pub const GAP_BUCKETS: usize = 41;
/// Count of measured gaps per bucket for one key (see [`Debouncer::gap_histogram`]).
pub type GapHistogram = [u32; GAP_BUCKETS];

/// Timing state is tracked per physical key: the virtual-key code plus the
/// extended flag (main Enter vs. numpad Enter, arrow cluster vs. numpad
/// arrows share a code). Counters and thresholds stay per virtual key.
pub const SLOTS: usize = 512;

/// Timing slot of a key: `vk`, or `vk + 256` for the extended variant.
pub fn slot_of(vk: usize, extended: bool) -> usize {
    (vk & 0xFF) | (usize::from(extended) << 8)
}

/// Longest allowed release delay (`iReleaseDeferMs`).
pub const RELEASE_DEFER_MAX_MS: u64 = 100;

/// What the hook does with one event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Forward the event.
    Pass,
    /// Drop the event.
    Block,
    /// Drop this release for now: it is sent later by
    /// [`Debouncer::due_releases`], or cancelled when the key bounces back
    /// down in time (release delay, `iReleaseDeferMs`).
    Defer,
    /// A held-back release is overdue (its timer fired late): drop this
    /// event, send the release first, then send this press again when
    /// `then_pass`.
    Flush { then_pass: bool },
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
    /// How long a release of a held key is held back (zero = off).
    release_defer: Duration,
    last_release: [Option<Duration>; SLOTS],
    last_press: [Option<Duration>; SLOTS],
    held: [bool; SLOTS],
    /// Release time of each held-back `Up` (see [`Decision::Defer`]).
    pending_up: [Option<Duration>; SLOTS],
    pub total_presses: u64,
    pub blocked_presses: u64,
    pub per_key_presses: [u64; 256],
    pub per_key_blocked: [u64; 256],
    /// (virtual-key code, observed gap) of the most recent block.
    pub last_blocked: Option<(u8, Duration)>,
    /// Keyboard cleaning mode: every press is blocked until Enter
    /// (VK_RETURN) is pressed; releases pass so no key stays "stuck down".
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
    /// Every measured gap (blocked or not) per key, for the threshold
    /// recommendation. Allocated once here; the hook only increments.
    gap_hist: Box<[GapHistogram; 256]>,
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
            release_defer: Duration::ZERO,
            last_release: [None; SLOTS],
            last_press: [None; SLOTS],
            held: [false; SLOTS],
            pending_up: [None; SLOTS],
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
            gap_hist: vec![[0; GAP_BUCKETS]; 256]
                .into_boxed_slice()
                .try_into()
                .expect("256 histograms"),
        }
    }

    /// Gaps measured for `vk` since startup or the last statistics reset.
    pub fn gap_histogram(&self, vk: u8) -> &GapHistogram {
        &self.gap_hist[vk as usize]
    }

    fn record_gap(&mut self, vk: usize, gap: Duration) {
        let i = ((gap.as_millis() as u64 / GAP_BUCKET_MS) as usize).min(GAP_BUCKETS - 1);
        let slot = &mut self.gap_hist[vk][i];
        *slot = slot.saturating_add(1);
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
        for h in self.gap_hist.iter_mut() {
            *h = [0; GAP_BUCKETS];
        }
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

    pub fn release_defer_ms(&self) -> u64 {
        self.release_defer.as_millis() as u64
    }

    /// Hold back each release of a held key this long (0 turns it off).
    /// Releases already held back are sent by the next
    /// [`Debouncer::due_releases`] when the delay shrinks.
    pub fn set_release_defer_ms(&mut self, ms: u64) {
        self.release_defer = Duration::from_millis(ms.min(RELEASE_DEFER_MAX_MS));
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

    fn log_passed(&mut self, vk: usize, now: Duration, gap: Option<Duration>) {
        if self.record_all_keys {
            self.push_log(LogEvent {
                time: now,
                vk: vk as u8,
                blocked: false,
                gap_ms: gap.map(|g| g.as_millis() as u64),
            });
        }
    }

    fn note_block(&mut self, vk: usize, now: Duration, gap: Duration) {
        self.blocked_presses += 1;
        self.per_key_blocked[vk] += 1;
        self.last_blocked = Some((vk as u8, gap));
        self.push_log(LogEvent {
            time: now,
            vk: vk as u8,
            blocked: true,
            gap_ms: Some(gap.as_millis() as u64),
        });
    }

    /// Returns `true` when the event should reach applications (a key with
    /// no extended variant and no release delay in play).
    #[cfg(test)]
    pub fn decide(&mut self, vk: usize, kind: KeyEventKind, now: Duration) -> bool {
        self.decide_event(vk, false, kind, now) == Decision::Pass
    }

    /// Classify one key or mouse-button event.
    pub fn decide_event(
        &mut self,
        vk: usize,
        extended: bool,
        kind: KeyEventKind,
        now: Duration,
    ) -> Decision {
        let slot = slot_of(vk, extended);
        let vk = vk & 0xFF;

        // Cleaning mode: block every press; Enter (0x0D) ends it. Releases
        // pass, so a key held when cleaning started is not left stuck down.
        if self.cleaning_mode {
            if kind == KeyEventKind::Down && vk == 0x0D {
                self.cleaning_mode = false;
            }
            return match kind {
                KeyEventKind::Up => Decision::Pass,
                KeyEventKind::Down => Decision::Block,
            };
        }

        // Shared codes for many different characters/keys: never chatter.
        if vk == VK_PACKET || vk == VK_PROCESSKEY {
            return Decision::Pass;
        }

        match kind {
            KeyEventKind::Up => {
                if self.pending_up[slot].is_some() {
                    // A second release while one is held back: the first
                    // still stands for both.
                    return Decision::Block;
                }
                if !self.release_defer.is_zero() && self.held[slot] {
                    self.pending_up[slot] = Some(now);
                    return Decision::Defer;
                }
                self.held[slot] = false;
                self.last_release[slot] = Some(now);
                Decision::Pass
            }
            KeyEventKind::Down => {
                if let Some(released) = self.pending_up[slot].take() {
                    let gap = now.saturating_sub(released);
                    if gap < self.release_defer {
                        // The contact bounced while the key is held: the
                        // release was never shown, so the key simply stays
                        // down for applications.
                        self.total_presses += 1;
                        self.per_key_presses[vk] += 1;
                        self.record_gap(vk, gap);
                        self.note_block(vk, now, gap);
                        return Decision::Block;
                    }
                    // The timer was late: release first, then judge the
                    // press as usual.
                    self.held[slot] = false;
                    self.last_release[slot] = Some(released);
                    let then_pass = self.judge_down(slot, vk, now);
                    return Decision::Flush { then_pass };
                }
                if self.judge_down(slot, vk, now) {
                    Decision::Pass
                } else {
                    Decision::Block
                }
            }
        }
    }

    /// The press half of the filter: `true` forwards the press.
    fn judge_down(&mut self, slot: usize, vk: usize, now: Duration) -> bool {
        self.total_presses += 1;
        self.per_key_presses[vk] += 1;

        // No release in between: genuine auto-repeat, never chatter.
        if self.held[slot] {
            self.last_press[slot] = Some(now);
            self.log_passed(vk, now, None);
            return true;
        }

        let anchor = match self.measure_from {
            MeasureFrom::Release => self.last_release[slot],
            MeasureFrom::Press => self.last_press[slot],
        };

        if let Some(prev) = anchor {
            let gap = now.saturating_sub(prev);
            self.record_gap(vk, gap);
            if gap < self.min_chatter {
                // Bugged input (e.g. 0 ms duplicate): not chatter.
                self.held[slot] = true;
                self.last_press[slot] = Some(now);
                self.log_passed(vk, now, Some(gap));
                return true;
            }
            if gap < self.effective_threshold(vk) {
                // The switch is physically down again: a further Down
                // before the next Up is the keyboard's auto-repeat of a
                // held key (a bounce mid-hold), not another chatter, and
                // must not be blocked.
                self.held[slot] = true;
                self.note_block(vk, now, gap);
                return false;
            }
        }

        self.held[slot] = true;
        self.last_press[slot] = Some(now);
        self.log_passed(vk, now, None);
        true
    }

    /// Whether any release is being held back.
    pub fn has_pending_releases(&self) -> bool {
        self.pending_up.iter().any(Option::is_some)
    }

    /// Releases whose delay ran out by `now` (all of them when `flush_all`):
    /// they become real releases here and their slots are written to `out`
    /// for the caller to send. Returns how many were written.
    pub fn due_releases(
        &mut self,
        now: Duration,
        flush_all: bool,
        out: &mut [u16; SLOTS],
    ) -> usize {
        let mut n = 0;
        for slot in 0..SLOTS {
            let Some(released) = self.pending_up[slot] else {
                continue;
            };
            if flush_all || now.saturating_sub(released) >= self.release_defer {
                self.pending_up[slot] = None;
                self.held[slot] = false;
                self.last_release[slot] = Some(released);
                out[n] = slot as u16;
                n += 1;
            }
        }
        n
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
    fn held_key_bounce_does_not_count_repeats_as_chatter() {
        // A held key whose contact bounces once (Up, Down) in the middle of
        // the hold: the bounce Down is chatter, but the keyboard's next
        // auto-repeat Down must resume the key, not be blocked as more
        // chatter (it used to be, until the gap since the bounce release
        // grew past the threshold).
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x57, KeyEventKind::Down, ms(0)));
        assert!(d.decide(0x57, KeyEventKind::Up, ms(200)));
        assert!(!d.decide(0x57, KeyEventKind::Down, ms(203)));
        assert!(d.decide(0x57, KeyEventKind::Down, ms(233)));
        assert!(d.decide(0x57, KeyEventKind::Down, ms(266)));
        assert_eq!(d.blocked_presses, 1);
        assert_eq!(d.per_key_blocked[0x57], 1);
    }

    #[test]
    fn release_chatter_is_still_blocked_after_a_blocked_bounce() {
        // Down, Up, bounce Down (blocked), bounce Up, bounce Down again:
        // the Up in between ends the "held" state, so the second bounce is
        // judged as chatter again.
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(0)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(100)));
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(104)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(107)));
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(111)));
        assert_eq!(d.blocked_presses, 2);
    }

    #[test]
    fn shared_ime_codes_always_pass_and_are_not_counted() {
        let mut d = Debouncer::new(50);
        d.record_all_keys = true;
        for vk in [VK_PACKET, VK_PROCESSKEY] {
            for t in 0..5 {
                // Different characters 2 ms apart, all reported as one code.
                assert!(d.decide(vk, KeyEventKind::Down, ms(t * 4)));
                assert!(d.decide(vk, KeyEventKind::Up, ms(t * 4 + 2)));
            }
        }
        assert_eq!((d.total_presses, d.blocked_presses), (0, 0));
        assert!(d.log_events.is_empty());
    }

    #[test]
    fn cleaning_mode_lets_releases_through() {
        // A key held while cleaning starts must not stay "down" in apps.
        let mut d = Debouncer::new(50);
        d.cleaning_mode = true;
        assert!(d.decide(0x10, KeyEventKind::Up, ms(10)));
        assert!(!d.decide(0x10, KeyEventKind::Down, ms(20)));
    }

    #[test]
    fn extended_and_plain_keys_sharing_a_code_are_timed_apart() {
        // Main Enter then numpad Enter 10 ms later: two physical keys.
        let mut d = Debouncer::new(50);
        let down = |d: &mut Debouncer, ext, t| d.decide_event(0x0D, ext, KeyEventKind::Down, ms(t));
        let up = |d: &mut Debouncer, ext, t| d.decide_event(0x0D, ext, KeyEventKind::Up, ms(t));
        assert_eq!(down(&mut d, false, 0), Decision::Pass);
        assert_eq!(up(&mut d, false, 30), Decision::Pass);
        assert_eq!(down(&mut d, true, 40), Decision::Pass);
        assert_eq!(up(&mut d, true, 60), Decision::Pass);
        // The same physical key bouncing is still chatter.
        assert_eq!(down(&mut d, true, 65), Decision::Block);
        // Counters stay per virtual key.
        assert_eq!((d.per_key_presses[0x0D], d.per_key_blocked[0x0D]), (3, 1));
    }

    fn defer(ms_: u64) -> Debouncer {
        let mut d = Debouncer::new(50);
        d.set_release_defer_ms(ms_);
        d
    }

    #[test]
    fn release_defer_hides_a_bounce_while_held() {
        let mut d = defer(30);
        let mut out = [0u16; SLOTS];
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Down, ms(0)),
            Decision::Pass
        );
        // Mid-hold bounce: the release is held back, the bounce press is
        // swallowed, and applications never see the key go up.
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Up, ms(200)),
            Decision::Defer
        );
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Down, ms(204)),
            Decision::Block
        );
        assert!(!d.has_pending_releases());
        assert_eq!(d.blocked_presses, 1);
        // Auto-repeat continues normally.
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Down, ms(233)),
            Decision::Pass
        );
        // The real release goes out once the delay ran out.
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Up, ms(500)),
            Decision::Defer
        );
        assert_eq!(d.due_releases(ms(520), false, &mut out), 0);
        assert_eq!(d.due_releases(ms(530), false, &mut out), 1);
        assert_eq!(out[0], 0x57);
        // Measured from the real release time: 540 is 40 ms after 500.
        assert_eq!(
            d.decide_event(0x57, false, KeyEventKind::Down, ms(540)),
            Decision::Block
        );
    }

    #[test]
    fn release_defer_flushes_a_late_release_before_the_next_press() {
        let mut d = defer(20);
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Down, ms(0)),
            Decision::Pass
        );
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Up, ms(100)),
            Decision::Defer
        );
        // The timer did not run: 300 ms later the release is still pending.
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Down, ms(300)),
            Decision::Flush { then_pass: true }
        );
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Up, ms(400)),
            Decision::Defer
        );
        // Late again, and this press is chatter after the real release.
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Down, ms(430)),
            Decision::Flush { then_pass: false }
        );
    }

    #[test]
    fn release_defer_only_holds_releases_of_held_keys() {
        let mut d = defer(30);
        // A release with no press seen (e.g. the key was down at startup).
        assert_eq!(
            d.decide_event(0x41, false, KeyEventKind::Up, ms(10)),
            Decision::Pass
        );
        // A duplicate release while one is held back is dropped.
        assert_eq!(
            d.decide_event(0x42, false, KeyEventKind::Down, ms(20)),
            Decision::Pass
        );
        assert_eq!(
            d.decide_event(0x42, false, KeyEventKind::Up, ms(30)),
            Decision::Defer
        );
        assert_eq!(
            d.decide_event(0x42, false, KeyEventKind::Up, ms(31)),
            Decision::Block
        );
        // Turning the delay off releases what is held back at once.
        d.set_release_defer_ms(0);
        let mut out = [0u16; SLOTS];
        assert_eq!(d.due_releases(ms(32), false, &mut out), 1);
        assert_eq!(
            d.decide_event(0x42, false, KeyEventKind::Down, ms(200)),
            Decision::Pass
        );
        assert_eq!(
            d.decide_event(0x42, false, KeyEventKind::Up, ms(250)),
            Decision::Pass
        );
        assert_eq!(d.release_defer_ms(), 0);
        d.set_release_defer_ms(5000);
        assert_eq!(d.release_defer_ms(), RELEASE_DEFER_MAX_MS);
    }

    #[test]
    fn release_defer_never_loses_a_release() {
        // Random streams with a delay on: every held-back release is either
        // cancelled by a bounce or sent, and a final flush leaves none.
        for seed in 0..100u64 {
            let mut rng = Lcg(seed);
            let mut d = defer(5 + rng.next() % 40);
            let mut out = [0u16; SLOTS];
            let mut now = 0u64;
            for _ in 0..300 {
                now += rng.next() % 50;
                if rng.next().is_multiple_of(4) {
                    d.due_releases(ms(now), false, &mut out);
                }
                let vk = [0x41usize, 0x01][(rng.next() % 2) as usize];
                let kind = if rng.next().is_multiple_of(2) {
                    KeyEventKind::Down
                } else {
                    KeyEventKind::Up
                };
                let decision = d.decide_event(vk, false, kind, ms(now));
                if kind == KeyEventKind::Down {
                    assert_ne!(decision, Decision::Defer, "seed {seed}");
                }
            }
            d.due_releases(ms(now), true, &mut out);
            assert!(!d.has_pending_releases(), "seed {seed}");
        }
    }

    #[test]
    fn cleaning_mode_still_blocks_shared_ime_codes() {
        let mut d = Debouncer::new(50);
        d.cleaning_mode = true;
        assert!(!d.decide(VK_PACKET, KeyEventKind::Down, ms(10)));
    }

    #[test]
    fn gap_histogram_buckets_every_measured_gap() {
        let mut d = Debouncer::new(50);
        assert!(d.decide(0x41, KeyEventKind::Down, ms(0)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(10)));
        // 3 ms after release: bucket 0, blocked.
        assert!(!d.decide(0x41, KeyEventKind::Down, ms(13)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
        // 72 ms: bucket 14, forwarded.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(92)));
        // Auto-repeat has no gap and is not measured.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(125)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(130)));
        // 5 s: overflow bucket.
        assert!(d.decide(0x41, KeyEventKind::Down, ms(5130)));
        let h = d.gap_histogram(0x41);
        assert_eq!((h[0], h[14], h[GAP_BUCKETS - 1]), (1, 1, 1));
        assert_eq!(h.iter().sum::<u32>(), 3);
        assert_eq!(d.gap_histogram(0x42).iter().sum::<u32>(), 0);
        d.reset_stats();
        assert_eq!(d.gap_histogram(0x41).iter().sum::<u32>(), 0);
    }

    /// Small deterministic generator, so the invariant test needs no extra
    /// dependency and every failure is reproducible from its seed.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    #[test]
    fn invariants_hold_for_random_event_streams() {
        let keys = [0x41usize, 0x42, 0x20, VK_PACKET];
        for seed in 0..200u64 {
            let mut rng = Lcg(seed);
            let mut d = Debouncer::new(1 + rng.next() % 120);
            d.set_min_chatter_ms(rng.next() % 6);
            d.set_measure_from(if seed % 2 == 0 {
                MeasureFrom::Release
            } else {
                MeasureFrom::Press
            });
            d.record_all_keys = seed % 3 == 0;
            d.set_log_limit(10);
            let mut now = 0u64;
            // Last event seen per key: None = never, Some(true) = Down.
            let mut last_down: [Option<bool>; 256] = [None; 256];
            for _ in 0..400 {
                now += rng.next() % 60;
                let vk = keys[(rng.next() % keys.len() as u64) as usize];
                let kind = if rng.next().is_multiple_of(2) {
                    KeyEventKind::Down
                } else {
                    KeyEventKind::Up
                };
                let passed = d.decide(vk, kind, ms(now));
                match kind {
                    KeyEventKind::Up => assert!(passed, "seed {seed}: Up blocked"),
                    KeyEventKind::Down => {
                        if last_down[vk].is_none() {
                            assert!(passed, "seed {seed}: first press blocked");
                        }
                        if last_down[vk] == Some(true) {
                            assert!(passed, "seed {seed}: auto-repeat blocked");
                        }
                        if vk == VK_PACKET {
                            assert!(passed, "seed {seed}: VK_PACKET blocked");
                        }
                    }
                }
                last_down[vk] = Some(kind == KeyEventKind::Down);
            }
            assert!(d.blocked_presses <= d.total_presses, "seed {seed}");
            assert_eq!(d.per_key_presses.iter().sum::<u64>(), d.total_presses);
            assert_eq!(d.per_key_blocked.iter().sum::<u64>(), d.blocked_presses);
            assert_eq!(d.per_key_presses[VK_PACKET], 0, "seed {seed}");
            assert!(d.log_events.len() <= d.log_limit(), "seed {seed}");
            for vk in keys {
                let measured: u32 = d.gap_histogram(vk as u8).iter().sum();
                assert!(u64::from(measured) <= d.per_key_presses[vk], "seed {seed}");
            }
        }
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
