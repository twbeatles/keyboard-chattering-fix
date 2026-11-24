//! Per-key threshold recommendation from the measured gap histogram, and
//! the switch-deterioration watch from the persisted day history.
//!
//! `advice` and `trend` hold the pure domain logic; this hub only
//! re-exports the public surface (mirrors `config.rs`/`updater.rs`).

mod advice;
mod trend;

pub use advice::{Advice, recommend};
pub use trend::{TREND_WINDOW_DAYS, window_sums, worst_keys, ymd_to_days};
