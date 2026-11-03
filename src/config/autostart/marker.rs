//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

use super::consts::{MARKER_VALUE, STATE_KEY};
use super::key::{Key, wide};
use windows::{
    Win32::System::Registry::{HKEY_CURRENT_USER, KEY_READ, REG_DWORD, RegDeleteKeyW},
    core::PCWSTR,
};

/// Whether `path` lies inside one of `roots` (whole path components,
/// case-insensitive, as Windows compares paths).
pub(super) fn is_under_any(path: &str, roots: &[String]) -> bool {
    let norm = |p: &str| {
        let p = p
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase();
        format!("{}\\", p.trim_end_matches('\\'))
    };
    let path = norm(path);
    roots
        .iter()
        .filter(|r| !r.trim().is_empty())
        .any(|root| path.starts_with(&norm(root)))
}

/// Folders only administrators can change: Program Files (both) and Windows.
pub(super) fn protected_roots() -> Vec<String> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Windows, KF_FLAG_DEFAULT,
            SHGetKnownFolderPath,
        },
    };
    [
        FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86,
        FOLDERID_Windows,
    ]
    .iter()
    .filter_map(|id| unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    })
    .collect()
}

/// Whether this executable sits where only administrators can replace it.
/// Anywhere else (a portable folder, Scoop, winget), a task that starts it
/// elevated at every sign-in lets any program of this account replace the
/// file and gain administrator rights (2026-10-22 audit ISSUE-003).
pub fn exe_in_protected_location() -> bool {
    std::env::current_exe()
        .is_ok_and(|exe| is_under_any(&exe.to_string_lossy(), &protected_roots()))
}

/// Drop the elevated-start marker (a non-elevated automatic start waited
/// for the elevated one in vain: the task is gone or disabled).
pub fn clear_elevated_start_marker() {
    set_marker(false);
}

/// Set the elevated-start marker (the elevated sign-in task just ran).
pub fn mark_elevated_start() {
    set_marker(true);
}

pub(super) fn set_marker(on: bool) {
    if on {
        if let Some(key) = Key::create(STATE_KEY) {
            let _ = key.set(MARKER_VALUE, REG_DWORD, &1u32.to_le_bytes());
        }
    } else {
        let path = wide(STATE_KEY);
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr())) };
    }
}

/// Whether an elevated sign-in start is registered, so a non-elevated
/// automatic start should give it a chance first. A plain registry read.
pub fn elevated_start_expected() -> bool {
    let Some(key) = Key::open(STATE_KEY, KEY_READ) else {
        return false;
    };
    let mut buf = [0u8; 4];
    key.query(MARKER_VALUE, &mut buf)
        .is_some_and(|(kind, len)| kind == REG_DWORD && len == 4 && buf != [0; 4])
}
