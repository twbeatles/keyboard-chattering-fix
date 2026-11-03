//! Home-tab statistics and header status, refreshed on the 500 ms timer.
//!
//! The timer does not touch windows blindly: the debouncer state is
//! snapshotted under a short lock, rendered to plain strings with no lock
//! held, and only controls whose text actually changed are updated. An idle
//! tick therefore performs zero `SetWindowTextW` calls.
//!
//! One responsibility per submodule (SRP):
//!
//! - `model`: snapshot/rendered/diff value types
//! - `render`: pure snapshot-to-strings rendering and frame diffing
//! - `tick`: reentrancy guard, last-frame cache, invalidation
//! - `update`: the 500 ms `update_stats` refresh

mod model;
mod render;
mod tick;
mod update;

#[cfg(test)]
mod tests;

pub(crate) use tick::invalidate;
pub(crate) use update::update_stats;
