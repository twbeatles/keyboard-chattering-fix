//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use std::path::PathBuf;

/// `settings.ini` next to the executable, when present: portable mode
/// (the file may start empty; the first save fills it).
pub(super) fn portable_path() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join("settings.ini");
    path.is_file().then_some(path)
}

pub fn config_path() -> PathBuf {
    if let Some(path) = portable_path() {
        return path;
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let modern = PathBuf::from(&base)
        .join("KeyboardChatteringFix")
        .join("settings.ini");
    if modern.exists() {
        return modern;
    }
    let legacy = PathBuf::from(&base).join("KcfixRs").join("settings.ini");
    if legacy.exists() {
        return legacy;
    }
    modern
}

/// Backup next to the settings file, used when the main file is torn.
pub(super) fn backup_path(path: &std::path::Path) -> PathBuf {
    path.with_extension("ini.bak")
}
