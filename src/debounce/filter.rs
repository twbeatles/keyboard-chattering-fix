//! Chatter-filter core: the `Debouncer` state and its accept/block decisions.
use super::types::{
    Decision, GAP_BUCKETS, GapHistogram, KeyEventKind, LogEvent, MOUSE_THRESHOLD_DEFAULT_MS,
    MeasureFrom, SLOTS, VK_PACKET, VK_PROCESSKEY, modifier_bit, slot_of,
};
use std::{collections::VecDeque, time::Duration};

/// Time-based chatter filter.
///
/// A `Down` is blocked when the same key fires again inside its threshold.
/// Auto-repeat (a `Down` with no `Up` in between) is always forwarded, so
/// holding a key never loses characters.
pub struct Debouncer {
    pub(crate) global_threshold: Duration,
    /// Used for mouse buttons without a per-key override.
    pub(crate) mouse_threshold: Duration,
    /// Zero means the key is never filtered.
    pub(crate) per_key: [Option<Duration>; 256],
    pub(crate) measure_from: MeasureFrom,
    pub(crate) min_chatter: Duration,
    /// How long a release of a held key is held back (zero = off).
    pub(crate) release_defer: Duration,
    pub(crate) last_release: [Option<Duration>; SLOTS],
    pub(crate) last_press: [Option<Duration>; SLOTS],
    pub(crate) held: [bool; SLOTS],
    /// Release time of each held-back `Up` (see [`Decision::Defer`]).
    pub(crate) pending_up: [Option<Duration>; SLOTS],
    /// Modifier keys held right now (see [`modifier_bit`]).
    pub(crate) mods: u8,
    /// Bumped whenever `mods` changes (a held modifier's auto-repeat does
    /// not count).
    pub(crate) mod_epoch: u32,
    /// `mod_epoch` at each slot's last release: a modifier pressed or
    /// released after it means the user typed on purpose (see `judge_down`).
    pub(crate) release_epoch: [u32; SLOTS],
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
    pub(crate) log_limit: usize,
    /// Bumped on every log or counter reset change, so the History tab can
    /// skip re-rendering when nothing new was recorded.
    pub log_seq: u64,
    /// Every measured gap (blocked or not) per key, for the threshold
    /// recommendation. Allocated once here; the hook only increments.
    pub(crate) gap_hist: Box<[GapHistogram; 256]>,
}

impl Debouncer {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            global_threshold: Duration::from_millis(threshold_ms.clamp(1, 1000)),
            mouse_threshold: Duration::from_millis(MOUSE_THRESHOLD_DEFAULT_MS),
            per_key: [None; 256],
            measure_from: MeasureFrom::Release,
            min_chatter: Duration::ZERO,
            release_defer: Duration::ZERO,
            last_release: [None; SLOTS],
            last_press: [None; SLOTS],
            held: [false; SLOTS],
            pending_up: [None; SLOTS],
            mods: 0,
            mod_epoch: 0,
            release_epoch: [0; SLOTS],
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

        // Modifier changes before this event: a modifier's own press is
        // not a change since its own release.
        let epoch = self.mod_epoch;
        let bit = modifier_bit(vk);
        let mods = match kind {
            KeyEventKind::Down => self.mods | bit,
            KeyEventKind::Up => self.mods & !bit,
        };
        if mods != self.mods {
            self.mods = mods;
            self.mod_epoch = self.mod_epoch.wrapping_add(1);
        }

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
                self.release_epoch[slot] = self.mod_epoch;
                // Keys that are never filtered are not held back either: a
                // late Shift release would shift the next key (`깨` -> `꺠`).
                if !self.release_defer.is_zero()
                    && self.held[slot]
                    && !self.effective_threshold(vk).is_zero()
                {
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
                    let then_pass = self.judge_down(slot, vk, epoch, now);
                    return Decision::Flush { then_pass };
                }
                if self.judge_down(slot, vk, epoch, now) {
                    Decision::Pass
                } else {
                    Decision::Block
                }
            }
        }
    }

    /// The press half of the filter: `true` forwards the press.
    fn judge_down(&mut self, slot: usize, vk: usize, epoch: u32, now: Duration) -> bool {
        self.total_presses += 1;
        self.per_key_presses[vk] += 1;
        let mods_changed = self.release_epoch[slot] != epoch;

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
            // A modifier went down or up after this key's release: the user
            // typed another character on purpose (`ㅆ` then `ㅅ` in
            // `있습니다`, or Shift let go and pressed again for `쌌`), which a
            // bouncing contact cannot do. Counted from the release, not the
            // press: Shift is often let go before the letter (Shift+T, Shift
            // up, T up), and a bounce after that must still be blocked.
            if gap < self.effective_threshold(vk) && !mods_changed {
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
}
