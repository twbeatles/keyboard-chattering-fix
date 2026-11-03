//! Per-key threshold recommendation from the measured gap histogram.
//!
//! `advice` holds the pure `Advice`/`recommend` domain logic; this hub only
//! re-exports the public surface (mirrors `config.rs`/`updater.rs`).

mod advice;

pub use advice::{Advice, recommend};
