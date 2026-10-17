//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use std::{collections::HashMap, fs, path::PathBuf};

use super::model::Settings;
use crate::{
    debounce::{MeasureFrom, LOG_LIMIT_MAX, LOG_LIMIT_MIN},
    i18n::Language,
};

pub fn config_path() -> PathBuf {
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
fn backup_path(path: &std::path::Path) -> PathBuf {
    path.with_extension("ini.bak")
}

pub fn load() -> Settings {
    try_load().unwrap_or_default()
}

/// Settings from disk, or `None` when neither the main file nor its backup
/// is readable. Callers that merge disk state into memory use this so an
/// unreadable file is never mistaken for "everything was removed".
pub fn try_load() -> Option<Settings> {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => Some(parse(&text)),
        _ => {
            // Missing/empty main file: a torn write may have truncated it,
            // so fall back to the previous generation (ISSUE-004).
            let bak = fs::read_to_string(backup_path(&path)).ok()?;
            (!bak.trim().is_empty()).then(|| parse(&bak))
        }
    }
}

fn parse(text: &str) -> Settings {
    let mut s = Settings::default();
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';', '[']) {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    if let Some(v) = map.get("iThreshold") {
        if let Ok(n) = v.parse::<u64>() {
            s.threshold_ms = n.clamp(1, 1000);
        }
    }
    if let Some(v) = map
        .get("bFilterEnabled")
        .or_else(|| map.get("bFilterStatus"))
    {
        s.enabled = v != "0";
    }
    if let Some(v) = map.get("bPlaySound") {
        s.play_sound = v != "0";
    }
    if let Some(v) = map.get("lpSoundFile") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            s.sound_file = Some(trimmed.to_string());
        }
    }
    if let Some(v) = map
        .get("bMinimizeOnLaunch")
        .or_else(|| map.get("bMinimizeWhenLaunch"))
    {
        s.minimize_on_launch = v == "1";
    }
    if let Some(v) = map.get("bCloseToTray") {
        s.close_to_tray = v != "0";
    }
    if let Some(v) = map.get("bRecordAllKeypress") {
        s.record_all_keys = v != "0";
    }
    if let Some(v) = map.get("iLogLimit") {
        if let Ok(n) = v.parse::<usize>() {
            s.log_limit = n.clamp(LOG_LIMIT_MIN, LOG_LIMIT_MAX);
        }
    }
    if let Some(v) = map.get("sLanguage") {
        s.language = Language::from_code(v);
    }
    if let Some(v) = map.get("sMeasureFrom") {
        s.measure_from = if v.eq_ignore_ascii_case("press") {
            MeasureFrom::Press
        } else {
            MeasureFrom::Release
        };
    }
    if let Some(v) = map.get("iMinChatterMs") {
        if let Ok(n) = v.parse::<u64>() {
            s.min_chatter_ms = n.min(1000);
        }
    }
    if let Some(v) = map.get("bExcludeInjected") {
        s.exclude_injected = v != "0";
    }
    if let Some(v) = map.get("iTotalPresses") {
        if let Ok(n) = v.parse::<u64>() {
            s.total_presses = n;
        }
    }
    if let Some(v) = map.get("iBlockedPresses") {
        if let Ok(n) = v.parse::<u64>() {
            s.blocked_presses = n;
        }
    }
    // Per-key cumulative counters: stat.<hex vk>=<presses>,<blocked>.
    // Only non-zero entries are written, so absence means zero.
    for (k, v) in &map {
        if let Some(hex) = k.strip_prefix("stat.") {
            if let Ok(vk) = u8::from_str_radix(hex, 16) {
                if let Some((presses, blocked)) = v.split_once(',') {
                    if let (Ok(p), Ok(b)) = (presses.parse::<u64>(), blocked.parse::<u64>()) {
                        s.per_key_presses[vk as usize] = p;
                        s.per_key_blocked[vk as usize] = b;
                    }
                }
            }
        }
    }
    // Per-key overrides: key.<2-digit hex virtual-key code>=<ms>.
    for (k, v) in &map {
        if let Some(hex) = k.strip_prefix("key.") {
            if let (Ok(vk), Ok(ms)) = (u8::from_str_radix(hex, 16), v.parse::<u64>()) {
                s.per_key_ms[vk as usize] = Some(ms.clamp(1, 1000));
            }
        }
    }
    s
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
        "[KeyboardChatteringFix]\niThreshold={}\nbFilterEnabled={}\nbPlaySound={}\nlpSoundFile={}\nbMinimizeOnLaunch={}\nbCloseToTray={}\nbRecordAllKeypress={}\niLogLimit={}\nsLanguage={}\nsMeasureFrom={}\niMinChatterMs={}\nbExcludeInjected={}\niTotalPresses={}\niBlockedPresses={}\n",
        s.threshold_ms,
        if s.enabled { 1 } else { 0 },
        if s.play_sound { 1 } else { 0 },
        s.sound_file.as_deref().unwrap_or(""),
        if s.minimize_on_launch { 1 } else { 0 },
        if s.close_to_tray { 1 } else { 0 },
        if s.record_all_keys { 1 } else { 0 },
        s.log_limit,
        s.language.code(),
        match s.measure_from {
            MeasureFrom::Press => "press",
            MeasureFrom::Release => "release",
        },
        s.min_chatter_ms,
        if s.exclude_injected { 1 } else { 0 },
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
    // Atomic write: stage to a temp file in the same directory, keep the
    // previous generation as .bak, then rename over the main file, so a
    // crash can never leave a torn settings.ini (ISSUE-004).
    let tmp = path.with_extension("ini.tmp");
    if fs::write(&tmp, &text).is_err() {
        return false;
    }
    if path.exists() {
        let _ = fs::copy(&path, backup_path(&path));
    }
    fs::rename(&tmp, &path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Run `f` with APPDATA pointed at a fresh temp dir. Process-global env
    /// mutation is serialized so parallel tests cannot observe each other.
    fn with_isolated_appdata(f: impl FnOnce(&std::path::Path)) {
        let _lock = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("APPDATA");
        let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("kcfix_cfgtest_{}_{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("APPDATA", &dir);
        f(&dir);
        match previous {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_clamps_aliases_and_hex_keys() {
        with_isolated_appdata(|dir| {
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
            std::fs::write(
                &ini,
                "[KeyboardChatteringFix]\niThreshold=5000\nbFilterStatus=0\n\
                 bPlaySound=1\nlpSoundFile=C:\\s.wav\niLogLimit=3\nsLanguage=EN\n\
                 sMeasureFrom=press\niMinChatterMs=2000\nbExcludeInjected=0\n\
                 iTotalPresses=7\niBlockedPresses=2\nkey.41=200\nkey.GG=50\n",
            )
            .unwrap();
            let s = load();
            assert_eq!(s.threshold_ms, 1000);
            assert!(!s.enabled);
            assert!(s.play_sound);
            assert_eq!(s.sound_file.as_deref(), Some(r"C:\s.wav"));
            assert_eq!(s.log_limit, 10);
            assert_eq!(s.language, Language::En);
            assert_eq!(s.measure_from, MeasureFrom::Press);
            assert_eq!(s.min_chatter_ms, 1000);
            assert!(!s.exclude_injected);
            assert_eq!((s.total_presses, s.blocked_presses), (7, 2));
            assert_eq!(s.per_key_ms[0x41], Some(200));
        });
    }

    #[test]
    fn load_zero_threshold_clamps_to_one() {
        with_isolated_appdata(|dir| {
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
            std::fs::write(&ini, "[KeyboardChatteringFix]\niThreshold=0\n").unwrap();
            assert_eq!(load().threshold_ms, 1);
        });
    }

    #[test]
    fn save_load_round_trip_preserves_everything() {
        with_isolated_appdata(|dir| {
            let mut per_key_ms = [None; 256];
            per_key_ms[0x20] = Some(80);
            let s = Settings {
                threshold_ms: 77,
                enabled: false,
                play_sound: true,
                sound_file: Some(r"D:\audio\blip.wav".to_string()),
                minimize_on_launch: true,
                close_to_tray: false,
                language: Language::En,
                per_key_ms,
                measure_from: MeasureFrom::Press,
                min_chatter_ms: 9,
                exclude_injected: false,
                record_all_keys: true,
                log_limit: 250,
                total_presses: 1234,
                blocked_presses: 56,
                per_key_presses: {
                    let mut counts = [0u64; 256];
                    counts[0x20] = 100;
                    counts
                },
                per_key_blocked: {
                    let mut counts = [0u64; 256];
                    counts[0x20] = 40;
                    counts
                },
            };
            assert!(save(&s));
            // No staging file may be left behind by the atomic write.
            assert!(!dir
                .join("KeyboardChatteringFix")
                .join("settings.ini.tmp")
                .exists());
            let back = load();
            assert_eq!(back.threshold_ms, 77);
            assert!(!back.enabled);
            assert!(back.play_sound);
            assert_eq!(back.sound_file.as_deref(), Some(r"D:\audio\blip.wav"));
            assert!(back.minimize_on_launch);
            assert!(!back.close_to_tray);
            assert_eq!(back.language, Language::En);
            assert_eq!(back.per_key_ms[0x20], Some(80));
            assert_eq!(back.measure_from, MeasureFrom::Press);
            assert_eq!(back.min_chatter_ms, 9);
            assert!(!back.exclude_injected);
            assert!(back.record_all_keys);
            assert_eq!(back.log_limit, 250);
            assert_eq!((back.total_presses, back.blocked_presses), (1234, 56));
            assert_eq!(
                (back.per_key_presses[0x20], back.per_key_blocked[0x20]),
                (100, 40)
            );
            assert_eq!(
                (back.per_key_presses[0x41], back.per_key_blocked[0x41]),
                (0, 0)
            );
        });
    }

    #[test]
    fn torn_main_file_falls_back_to_backup() {
        with_isolated_appdata(|dir| {
            let first = Settings {
                threshold_ms: 111,
                ..Default::default()
            };
            assert!(save(&first));
            let second = Settings {
                threshold_ms: 222,
                ..Default::default()
            };
            assert!(save(&second));
            assert_eq!(load().threshold_ms, 222);
            // Simulate a crash mid-write: truncated main file.
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            std::fs::write(&ini, "").unwrap();
            // Previous generation (111) is recovered, not defaults (50).
            assert_eq!(load().threshold_ms, 111);
        });
    }

    #[test]
    fn save_reports_failure_when_appdata_is_unusable() {
        // APPDATA pointing at a regular file: every directory creation and
        // write lands on an unusable path, so save must report false
        // instead of pretending success (ISSUE-004).
        let _lock = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("APPDATA");
        let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let blocker =
            std::env::temp_dir().join(format!("kcfix_cfgblock_{}_{seq}", std::process::id()));
        std::fs::write(&blocker, b"not-a-directory").unwrap();
        std::env::set_var("APPDATA", &blocker);
        assert!(!save(&Settings::default()));
        match previous {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
        let _ = std::fs::remove_file(&blocker);
    }

    #[test]
    fn load_treats_any_nonzero_boolean_as_true() {
        // All boolean flags share one rule: only "0" is false. A stray value
        // must never silently flip a protection-relevant flag off.
        with_isolated_appdata(|dir| {
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
            std::fs::write(
                &ini,
                "[KeyboardChatteringFix]\nbPlaySound=2\nbRecordAllKeypress=yes\nbFilterEnabled=0\n",
            )
            .unwrap();
            let s = load();
            assert!(s.play_sound);
            assert!(s.record_all_keys);
            assert!(!s.enabled);
        });
    }

    #[test]
    fn external_per_key_edit_survives_persisting_the_startup_snapshot() {
        // ISSUE-005: the app holds the settings it loaded at startup; the
        // user adds `key.41=120` to the file while it runs, then the app
        // persists on exit. The edit must survive, UI-owned fields must not.
        with_isolated_appdata(|dir| {
            let startup = Settings {
                threshold_ms: 60,
                ..Default::default()
            };
            assert!(save(&startup));
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            let mut text = std::fs::read_to_string(&ini).unwrap();
            text = text.replace("iThreshold=60", "iThreshold=999");
            text.push_str("key.41=120\n");
            std::fs::write(&ini, text).unwrap();

            let mut app = startup.clone();
            app.total_presses = 10;
            app.adopt_file_only_fields(&load());
            assert!(save(&app));

            let back = load();
            assert_eq!(back.per_key_ms[0x41], Some(120));
            assert_eq!(back.threshold_ms, 60, "UI-owned field keeps the app value");
            assert_eq!(back.total_presses, 10);
        });
    }

    #[test]
    fn removed_per_key_edit_is_not_resurrected() {
        // Deleting a `key.XX` line in the file while running sticks too.
        with_isolated_appdata(|dir| {
            let mut startup = Settings::default();
            startup.per_key_ms[0x20] = Some(80);
            assert!(save(&startup));
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            let text = std::fs::read_to_string(&ini)
                .unwrap()
                .replace("key.20=80\n", "");
            std::fs::write(&ini, text).unwrap();

            let mut app = startup.clone();
            app.adopt_file_only_fields(&load());
            assert!(save(&app));
            assert_eq!(load().per_key_ms[0x20], None);
        });
    }

    #[test]
    fn try_load_distinguishes_missing_from_empty_settings() {
        with_isolated_appdata(|_| {
            // Nothing on disk: no state to merge, not "all defaults".
            assert!(try_load().is_none());
            assert!(save(&Settings::default()));
            assert!(try_load().is_some());
        });
    }

    #[test]
    fn per_key_stats_round_trip() {
        // Non-zero per-key counters survive save/load so the top-chatter
        // diagnosis does not reset every restart; zero entries stay absent.
        with_isolated_appdata(|_| {
            let mut presses = [0u64; 256];
            let mut blocked = [0u64; 256];
            presses[0x41] = 30;
            blocked[0x41] = 7;
            let s = Settings {
                per_key_presses: presses,
                per_key_blocked: blocked,
                ..Default::default()
            };
            assert!(save(&s));
            let back = load();
            assert_eq!(
                (back.per_key_presses[0x41], back.per_key_blocked[0x41]),
                (30, 7)
            );
            assert_eq!(
                (back.per_key_presses[0x42], back.per_key_blocked[0x42]),
                (0, 0)
            );
        });
    }
}
