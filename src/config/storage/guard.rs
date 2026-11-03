//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use super::encoding::read_text;

/// Every key this program writes (`key.XX`/`stat.XX`/`hist.XX` aside).
pub(super) const KNOWN_KEYS: &[&str] = &[
    "iThreshold",
    "bFilterEnabled",
    "bFilterStatus",
    "bPlaySound",
    "lpSoundFile",
    "bMinimizeOnLaunch",
    "bMinimizeWhenLaunch",
    "bCloseToTray",
    "bRecordAllKeypress",
    "iLogLimit",
    "sLanguage",
    "sMeasureFrom",
    "iMinChatterMs",
    "bExcludeInjected",
    "bFilterMouse",
    "iMouseThreshold",
    "iReleaseDeferMs",
    "sAutoDisableApps",
    "bAutoDisableFullscreen",
    "sHotkeyToggle",
    "sHotkeyCleaning",
    "iCleaningTimeoutMin",
    "iTotalPresses",
    "iBlockedPresses",
    "sHistMeasure",
];

/// Whether `text` is this program's settings file: the section header or
/// at least one known key. A file torn by a crash (zero bytes, NULs) or
/// replaced by something else is not, and must never pass for a file
/// whose settings were all reset to their defaults.
pub(super) fn looks_like_settings(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line == "[KeyboardChatteringFix]"
            || line.split_once('=').is_some_and(|(k, _)| {
                let k = k.trim();
                KNOWN_KEYS.contains(&k)
                    || ["key.", "stat.", "hist."].iter().any(|p| k.starts_with(p))
            })
    })
}

/// A readable settings file at `path`, as text.
pub(super) fn read_settings(path: &std::path::Path) -> Option<String> {
    read_text(path).filter(|text| looks_like_settings(text))
}
