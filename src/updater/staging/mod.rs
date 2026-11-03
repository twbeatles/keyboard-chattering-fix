//! Single-flight update guard, helper resolution, staging, and launch.
//!
//! One responsibility per module (SRP):
//!
//! - `guard`: the process-global single-flight update flag
//! - `helper`: helper binary discovery and version-compatibility checks
//! - `stage`: staging-path layout, download staging, stale cleanup
//! - `launch`: helper command-line construction and process launch

mod guard;
mod helper;
mod launch;
mod stage;

#[cfg(test)]
mod tests;

pub use guard::{end_update, try_begin_update, update_in_progress};
pub use helper::{HELPER_FILENAME, describe_helper, resolve_helper};
pub use launch::{
    HELPER_EXIT_TIMEOUT_SECS, HELPER_LANG_ENV, helper_args, launch_helper, relaunch_args_from,
};
pub use stage::{
    apply_update, cleanup_stale_staging, discard_staged, prepare_update, stage_helper,
    unique_staging_path, unix_now,
};
