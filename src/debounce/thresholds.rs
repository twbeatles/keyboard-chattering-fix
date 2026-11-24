//! Filter configuration: thresholds, baselines, and release deferral.
use super::filter::Debouncer;
use super::types::{
    MeasureFrom, RELEASE_DEFER_MAX_MS, WHEEL_REVERSE_MS, WHEEL_VK, is_mouse_button,
    is_unfiltered_modifier,
};
use std::time::Duration;

impl Debouncer {
    /// Effective threshold for `vk` in ms and whether it is a per-key
    /// override (`key.XX` in the settings file). Without an override the
    /// value is whatever currently filters the key: the app profile when a
    /// profiled app is in front, else the mouse window, the zero modifier
    /// rule, or the global window. 0 means "never filtered".
    pub fn key_threshold_ms(&self, vk: u8) -> (u64, bool) {
        match self.per_key[vk as usize] {
            Some(d) => (d.as_millis() as u64, true),
            None => (
                self.effective_threshold(vk as usize).as_millis() as u64,
                false,
            ),
        }
    }

    /// Block window while a profiled app is in front (`None` = none).
    pub fn set_app_threshold(&mut self, ms: Option<u64>) {
        self.app_threshold = ms.map(|m| Duration::from_millis(m.min(1000)));
    }

    pub fn app_threshold_ms(&self) -> Option<u64> {
        self.app_threshold.map(|d| d.as_millis() as u64)
    }

    /// Block window of mouse buttons without their own (`iMouseThreshold`).
    pub fn set_mouse_threshold_ms(&mut self, ms: u64) {
        self.mouse_threshold = Duration::from_millis(ms.clamp(1, 1000));
    }

    pub fn mouse_threshold_ms(&self) -> u64 {
        self.mouse_threshold.as_millis() as u64
    }

    pub fn measure_from(&self) -> MeasureFrom {
        self.measure_from
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
    /// `None` as `ms` clears the override; `Some(0)` never filters the key
    /// (its presses are still counted).
    pub fn set_key_threshold(&mut self, vk: u8, ms: Option<u64>) {
        self.per_key[vk as usize] = ms.map(|m| Duration::from_millis(m.min(1000)));
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

    pub(crate) fn effective_threshold(&self, vk: usize) -> Duration {
        match self.per_key[vk] {
            Some(d) => d,
            // The front app's window beats the device defaults, but never a
            // per-key override. A profile of 0 ms never filters (it still
            // counts presses), like `key.XX=0`.
            None => self
                .app_threshold
                .unwrap_or_else(|| self.base_threshold(vk)),
        }
    }

    /// Threshold without any app profile: the wheel window, the mouse
    /// window, the zero-modifier rule, or the global window.
    fn base_threshold(&self, vk: usize) -> Duration {
        if vk == WHEEL_VK {
            Duration::from_millis(WHEEL_REVERSE_MS)
        } else if is_mouse_button(vk) {
            self.mouse_threshold
        } else if is_unfiltered_modifier(vk) {
            Duration::ZERO
        } else {
            self.global_threshold
        }
    }
}
