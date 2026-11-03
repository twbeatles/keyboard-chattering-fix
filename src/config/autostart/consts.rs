//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

pub(super) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub(super) const RUN_VALUE: &str = "KeyboardChatteringFix";
/// Task Manager's Startup tab (and Settings > Apps > Startup) disables an
/// entry without removing it: `StartupApproved\Run` holds a binary value of
/// the same name whose first byte is odd (`03`) when disabled.
pub(super) const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Program state outside the settings file: only the elevated-task marker.
pub(super) const STATE_KEY: &str = r"Software\KeyboardChatteringFix";
pub(super) const MARKER_VALUE: &str = "ElevatedTask";

/// Command-line flag of automatic starts (see the module docs).
pub const AUTOSTART_ARG: &str = "--autostart";
/// Name of the scheduled task for the elevated start.
pub(super) const TASK_NAME: &str = "KeyboardChatteringFix";
