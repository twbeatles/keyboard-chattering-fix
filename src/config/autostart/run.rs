//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

use std::path::Path;

use super::consts::{APPROVED_KEY, AUTOSTART_ARG, RUN_KEY, RUN_VALUE};
use super::key::{Key, wide};
use windows::Win32::System::Registry::{KEY_READ, KEY_WRITE, REG_SZ};

/// Quote an executable path for the `Run` value (ISSUE-002).
pub(super) fn quote_run_path(exe: &str) -> String {
    format!("\"{exe}\" {AUTOSTART_ARG}")
}

/// The `Run` value as stored, if any.
pub(super) fn run_value() -> Option<String> {
    let key = Key::open(RUN_KEY, KEY_READ)?;
    let mut buf = [0u8; 4096];
    let (_, len) = key.query(RUN_VALUE, &mut buf)?;
    let units: Vec<u16> = buf[..len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// Executable path of a stored `Run` command line (quoted or not).
pub(super) fn registered_exe(stored: &str) -> &str {
    let path = stored.trim();
    match path.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => path.split(" --").next().unwrap_or(path),
    }
    .trim()
}

/// Whether a stored `Run` value starts *this* executable but predates the
/// `--autostart` flag. Entries for another copy are never touched.
pub(super) fn needs_flag_upgrade(stored: &str, exe: &str) -> bool {
    registered_exe(stored).eq_ignore_ascii_case(exe) && !stored.contains(AUTOSTART_ARG)
}

/// Add `--autostart` to an older startup entry of this executable. Only
/// the command line changes: a Task Manager "Disabled" mark stays.
pub fn upgrade_autostart_entry() {
    let (Some(stored), Ok(exe)) = (run_value(), std::env::current_exe()) else {
        return;
    };
    if needs_flag_upgrade(&stored, &exe.to_string_lossy()) {
        let _ = write_run_value();
    }
}

pub(super) fn approved_says_disabled(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 1 == 1)
}

pub(super) fn startup_disabled_by_user() -> bool {
    let Some(key) = Key::open(APPROVED_KEY, KEY_READ) else {
        return false;
    };
    let mut buf = [0u8; 16];
    key.query(RUN_VALUE, &mut buf)
        .is_some_and(|(_, len)| approved_says_disabled(&buf[..len]))
}

/// Forget the Task Manager mark (a missing value counts as enabled).
pub(super) fn clear_startup_approved() {
    if let Some(key) = Key::open(APPROVED_KEY, KEY_WRITE) {
        let _ = key.delete(RUN_VALUE);
    }
}

/// Whether Windows will start the program at sign-in through the `Run`
/// value: present and not disabled in Task Manager.
pub fn autostart_enabled() -> bool {
    run_value().is_some_and(|v| !v.trim().is_empty()) && !startup_disabled_by_user()
}

/// Apply the startup toggle of the settings form. Writes only when the
/// state differs, or when the entry starts an executable that no longer
/// exists — so saving from another copy (a test build) never takes over a
/// working entry of the installed one.
pub fn sync_autostart(enable: bool) -> bool {
    let target_exists = || run_value().is_some_and(|v| Path::new(registered_exe(&v)).is_file());
    if enable == autostart_enabled() && (!enable || target_exists()) {
        return true;
    }
    set_autostart(enable)
}

pub(super) fn set_autostart(enable: bool) -> bool {
    if !enable {
        let Some(key) = Key::open(RUN_KEY, KEY_WRITE) else {
            return false;
        };
        // A stale "Disabled" mark would silently override a later enable
        // made by another tool or an older version.
        clear_startup_approved();
        return key.delete(RUN_VALUE);
    }
    // An explicit enable also lifts a Task Manager "Disabled" mark, or the
    // entry would be written but never run.
    let ok = write_run_value();
    if ok {
        clear_startup_approved();
    }
    ok
}

/// Write the `Run` value for this executable.
pub(super) fn write_run_value() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(key) = Key::create(RUN_KEY) else {
        return false;
    };
    // Quote so install locations containing spaces keep launching
    // (ISSUE-002). The NUL terminator is included in the byte count.
    let bytes: Vec<u8> = wide(&quote_run_path(&exe.to_string_lossy()))
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    key.set(RUN_VALUE, REG_SZ, &bytes)
}

// ---- Elevated sign-in task -------------------------------------------------
