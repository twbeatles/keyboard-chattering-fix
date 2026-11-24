//! Time-based chatter filter core.
//!
//! One responsibility per module (SRP):
//!
//! - `types`: shared value types (events, thresholds, key helpers)
//! - `filter`: the `Debouncer` state and its accept/block decisions
//! - `thresholds`: threshold/baseline/release-defer configuration
//! - `stats`: counters, event log, gap histograms, top-chatter query

mod filter;
mod stats;
mod thresholds;
mod types;

#[cfg(test)]
mod tests;

pub use filter::Debouncer;
pub use stats::{LOG_LIMIT_MAX, LOG_LIMIT_MIN};
pub use types::{
    Decision, GAP_BUCKET_MS, GAP_BUCKETS, GapHistogram, KeyEventKind, LogEvent,
    MOUSE_THRESHOLD_DEFAULT_MS, MeasureFrom, RELEASE_DEFER_MAX_MS, SLOTS, THRESHOLD_DEFAULT_MS,
    WHEEL_VK, WheelAxis, slot_of,
};
