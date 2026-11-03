//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use std::fs;

use super::super::model::Settings;
use super::guard::read_settings;
use super::parse::parse;
use super::paths::{backup_path, config_path};
use super::serialize::{histogram_lines, measure_from_value, one_line};

pub fn load() -> Settings {
    try_load().unwrap_or_default()
}

/// Settings from any file (Import). `None` when it cannot be read or does
/// not look like this program's settings file.
pub fn load_from(path: &std::path::Path) -> Option<Settings> {
    read_settings(path).map(|text| parse(&text))
}

/// Settings from disk, or `None` when neither the main file nor its backup
/// is readable. Callers that merge disk state into memory use this so an
/// unreadable file is never mistaken for "everything was removed".
pub fn try_load() -> Option<Settings> {
    let path = config_path();
    // A missing, empty or unrecognizable main file (a write torn by a power
    // loss leaves zeros) falls back to the previous generation (ISSUE-004,
    // ISSUE-002 of the 2026-10-22 audit).
    read_settings(&path)
        .or_else(|| {
            let bak = read_settings(&backup_path(&path));
            if bak.is_some() && path.exists() {
                tracing::warn!("settings file unreadable; using its backup");
            }
            bak
        })
        .map(|text| parse(&text))
}

/// Persist settings atomically (temp file + `.bak` + rename).
/// Returns whether the settings reached disk; callers must surface `false`
/// instead of silently showing unsaved state as saved (ISSUE-004).
pub fn save(s: &Settings) -> bool {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut text = format!(
        "[KeyboardChatteringFix]\niThreshold={}\nbFilterEnabled={}\nbPlaySound={}\nlpSoundFile={}\nbMinimizeOnLaunch={}\nbCloseToTray={}\nbRecordAllKeypress={}\niLogLimit={}\nsLanguage={}\nsMeasureFrom={}\niMinChatterMs={}\nbExcludeInjected={}\nbFilterMouse={}\niMouseThreshold={}\niReleaseDeferMs={}\nsAutoDisableApps={}\nbAutoDisableFullscreen={}\nsHotkeyToggle={}\nsHotkeyCleaning={}\niCleaningTimeoutMin={}\niTotalPresses={}\niBlockedPresses={}\n",
        s.threshold_ms,
        if s.enabled { 1 } else { 0 },
        if s.play_sound { 1 } else { 0 },
        s.sound_file.as_deref().unwrap_or(""),
        if s.minimize_on_launch { 1 } else { 0 },
        if s.close_to_tray { 1 } else { 0 },
        if s.record_all_keys { 1 } else { 0 },
        s.log_limit,
        s.language.setting_value(),
        measure_from_value(s.measure_from),
        s.min_chatter_ms,
        if s.exclude_injected { 1 } else { 0 },
        if s.filter_mouse { 1 } else { 0 },
        s.mouse_threshold_ms,
        s.release_defer_ms,
        one_line(&s.auto_disable_apps),
        if s.auto_disable_fullscreen { 1 } else { 0 },
        one_line(&s.hotkey_toggle),
        one_line(&s.hotkey_cleaning),
        s.cleaning_timeout_min,
        s.total_presses,
        s.blocked_presses,
    );
    for (vk, ms) in s.per_key_ms.iter().enumerate() {
        if let Some(ms) = ms {
            text.push_str(&format!("key.{vk:02X}={ms}\n"));
        }
    }
    for (vk, (&presses, &blocked)) in s
        .per_key_presses
        .iter()
        .zip(s.per_key_blocked.iter())
        .enumerate()
    {
        if presses > 0 || blocked > 0 {
            text.push_str(&format!("stat.{vk:02X}={presses},{blocked}\n"));
        }
    }
    // Long and machine-written: last, behind a comment, so hand edits of the
    // lines above stay easy.
    let hist = histogram_lines(s);
    if !hist.is_empty() {
        text.push_str(&format!(
            "; Measured key gaps for the threshold suggestion (written by the program)\nsHistMeasure={}\n{hist}",
            measure_from_value(s.measure_from)
        ));
    }
    // Atomic write: stage to a temp file in the same directory, keep the
    // previous generation as .bak, then rename over the main file, so a
    // crash can never leave a torn settings.ini (ISSUE-004).
    let tmp = path.with_extension("ini.tmp");
    if !write_durably(&tmp, text.as_bytes()) {
        return false;
    }
    // Only a good generation becomes the backup: copying a main file torn
    // by an earlier crash would destroy the one copy that still works.
    if read_settings(&path).is_some() {
        let _ = fs::copy(&path, backup_path(&path));
    }
    replace_durably(&tmp, &path)
}

/// Write `bytes` and flush them to the disk before returning. Without the
/// flush, NTFS may journal a later rename while the data is still cached:
/// a power loss then leaves a file of the right size full of zeros.
pub(super) fn write_durably(path: &std::path::Path, bytes: &[u8]) -> bool {
    use std::io::Write;
    fs::File::create(path)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .is_ok()
}

/// Rename `from` over `to`, returning only once the rename is on disk.
pub(super) fn replace_durably(from: &std::path::Path, to: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let wide = |p: &std::path::Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let (from, to) = (wide(from), wide(to));
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .is_ok()
    }
}
