//! Windows startup integration: Run key, approved-startup state, marker.
mod consts;
mod key;
mod marker;
mod run;
mod task;
#[cfg(test)]
mod tests;
pub use consts::AUTOSTART_ARG;
pub use marker::{
    clear_elevated_start_marker, elevated_start_expected, exe_in_protected_location,
    mark_elevated_start,
};
pub use run::{autostart_enabled, sync_autostart, upgrade_autostart_entry};
pub use task::{TaskError, refresh_elevated_task, set_elevated_task};
