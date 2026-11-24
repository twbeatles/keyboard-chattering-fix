//! Filter statistics: counters, event log, gap histograms, top-chatter query.
use super::filter::Debouncer;
use super::types::{GAP_BUCKET_MS, GAP_BUCKETS, GapHistogram, LogEvent};
use std::time::Duration;

/// Bounds of the in-memory event log (UI and settings file share them).
pub const LOG_LIMIT_MIN: usize = 10;
pub const LOG_LIMIT_MAX: usize = 5000;

impl Debouncer {
    /// Gaps measured for `vk` since the last statistics reset (kept across
    /// restarts in the settings file).
    pub fn gap_histogram(&self, vk: u8) -> &GapHistogram {
        &self.gap_hist[vk as usize]
    }

    /// Every key's measured gaps (copied into the settings on save).
    pub fn gap_histograms(&self) -> &[GapHistogram; 256] {
        &self.gap_hist
    }

    /// Adopt the measured gaps saved by an earlier run (startup).
    pub fn set_gap_histograms(&mut self, hist: &[GapHistogram; 256]) {
        *self.gap_hist = *hist;
    }

    /// Forget every measured gap: a gap measured from the other baseline
    /// ([`MeasureFrom`]) means something else and must not be mixed in.
    /// Counters are kept.
    pub fn clear_gap_histograms(&mut self) {
        for h in self.gap_hist.iter_mut() {
            *h = [0; GAP_BUCKETS];
        }
    }

    pub(crate) fn record_gap(&mut self, vk: usize, gap: Duration) {
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
        self.day_presses = [0; 256];
        self.day_blocked = [0; 256];
        self.day_date = 0;
        self.last_blocked = None;
        self.last_wheel_v = None;
        self.last_wheel_h = None;
        for h in self.gap_hist.iter_mut() {
            *h = [0; GAP_BUCKETS];
        }
        self.clear_log();
    }

    /// Forget one key's counters, measured gaps, and log rows (per-key
    /// statistics reset, e.g. after replacing a switch). Global totals are
    /// decremented to match, saturating at zero. Thresholds are kept. The
    /// not-yet-saved day slice (at most one autosave interval) is dropped
    /// with them.
    pub fn clear_key_stats(&mut self, vk: u8) {
        let i = vk as usize;
        self.total_presses = self.total_presses.saturating_sub(self.per_key_presses[i]);
        self.blocked_presses = self.blocked_presses.saturating_sub(self.per_key_blocked[i]);
        self.per_key_presses[i] = 0;
        self.per_key_blocked[i] = 0;
        self.day_presses[i] = 0;
        self.day_blocked[i] = 0;
        self.gap_hist[i] = [0; GAP_BUCKETS];
        if vk as usize == crate::debounce::WHEEL_VK {
            self.last_wheel_v = None;
            self.last_wheel_h = None;
        }
        if self.last_blocked.is_some_and(|(b, _)| b == vk) {
            self.last_blocked = None;
        }
        self.log_events.retain(|e| e.vk != vk);
        self.log_seq += 1;
    }

    pub(crate) fn push_log(&mut self, event: LogEvent) {
        if self.log_events.len() >= self.log_limit {
            self.log_events.pop_front();
        }
        self.log_events.push_back(event);
        self.log_seq += 1;
    }

    pub(crate) fn log_passed(&mut self, vk: usize, now: Duration, gap: Option<Duration>) {
        if self.record_all_keys {
            self.push_log(LogEvent {
                time: now,
                vk: vk as u8,
                blocked: false,
                gap_ms: gap.map(|g| g.as_millis() as u64),
            });
        }
    }

    pub(crate) fn note_block(&mut self, vk: usize, now: Duration, gap: Duration) {
        self.blocked_presses += 1;
        self.per_key_blocked[vk] += 1;
        self.day_blocked[vk] += 1;
        self.last_blocked = Some((vk as u8, gap));
        self.push_log(LogEvent {
            time: now,
            vk: vk as u8,
            blocked: true,
            gap_ms: Some(gap.as_millis() as u64),
        });
    }

    /// Finds the key with the highest number of blocked chatter events.
    pub fn top_chattered_key(&self) -> Option<(u8, u64, f64)> {
        let mut best_vk = None;
        let mut max_blocked = 0;
        for (vk, &blocked) in self.per_key_blocked.iter().enumerate().take(256) {
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
