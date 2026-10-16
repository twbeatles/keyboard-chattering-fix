//! Application lifecycle: startup orchestration, CLI entry, single-instance
//! guard, and process-wide shared state.

pub(crate) mod bootstrap;
pub(crate) mod cli;
mod instance;
pub(crate) mod state;
