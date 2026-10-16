use std::{collections::HashMap, fs, path::PathBuf};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
        },
    },
};

use crate::{debounce::MeasureFrom, i18n::Language};

/// Application version. Single source of truth for the update pipeline:
/// must match `[package] version` in `Cargo.toml` and the `v{VERSION}`
/// GitHub Release tag. `tests/version_sync.rs` enforces this.
pub const VERSION: &str = "0.1.0";
/// Ed25519 public key (base64, 32 bytes) that verifies `update.json`.
/// The matching private key lives only as the `KCFIX_UPDATE_PRIVATE_KEY_B64`
/// GitHub Actions secret. See `docs/UPDATE.md`.
pub const UPDATE_PUBLIC_KEY_B64: &str = "YFczqFoqyNDWEXJrbbgLvJoIOMrNT3q0pigHPnYI0hU=";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "KeyboardChatteringFix";

#[derive(Clone, Debug)]
pub struct Settings {
    pub threshold_ms: u64,
    pub enabled: bool,
    pub play_sound: bool,
    pub sound_file: Option<String>,
    pub minimize_on_launch: bool,
    pub close_to_tray: bool,
    pub language: Language,
    /// Per-key threshold overrides in ms, indexed by virtual-key code.
    pub per_key_ms: [Option<u64>; 256],
    pub measure_from: MeasureFrom,
    pub min_chatter_ms: u64,
    pub exclude_injected: bool,
    pub record_all_keys: bool,
    pub log_limit: usize,
    pub total_presses: u64,
    pub blocked_presses: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_ms: 50,
            enabled: true,
            play_sound: false,
            sound_file: None,
            minimize_on_launch: false,
            close_to_tray: true,
            language: Language::Ko,
            per_key_ms: [None; 256],
            measure_from: MeasureFrom::Release,
            min_chatter_ms: 0,
            exclude_injected: true,
            record_all_keys: false,
            log_limit: 100,
            total_presses: 0,
            blocked_presses: 0,
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

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

/// Validate a configured sound path once, outside the keyboard hook.
/// Returns the path only when it points at an existing file, so the hook
/// never performs filesystem I/O per blocked keystroke (ISSUE-003).
pub fn resolve_sound_file(raw: Option<&str>) -> Option<PathBuf> {
    let raw = raw.filter(|s| !s.trim().is_empty())?;
    let path = PathBuf::from(raw.trim());
    path.is_file().then_some(path)
}

/// Backup next to the settings file, used when the main file is torn.
fn backup_path(path: &std::path::Path) -> PathBuf {
    path.with_extension("ini.bak")
}

pub fn load() -> Settings {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => parse(&text),
        _ => {
            // Missing/empty main file: a torn write may have truncated it,
            // so fall back to the previous generation (ISSUE-004).
            if let Ok(bak) = fs::read_to_string(backup_path(&path)) {
                if !bak.trim().is_empty() {
                    return parse(&bak);
                }
            }
            Settings::default()
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
        s.play_sound = v == "1";
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
        s.record_all_keys = v == "1";
    }
    if let Some(v) = map.get("iLogLimit") {
        if let Ok(n) = v.parse::<usize>() {
            s.log_limit = n.clamp(10, 5000);
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

pub fn save(s: &Settings) {
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
    // Atomic write: stage to a temp file in the same directory, keep the
    // previous generation as .bak, then rename over the main file, so a
    // crash can never leave a torn settings.ini (ISSUE-004).
    let tmp = path.with_extension("ini.tmp");
    if fs::write(&tmp, &text).is_err() {
        return;
    }
    if path.exists() {
        let _ = fs::copy(&path, backup_path(&path));
    }
    let _ = fs::rename(&tmp, &path);
}

/// Quote an executable path for the `Run` value (ISSUE-002).
fn quote_run_path(exe: &str) -> String {
    format!("\"{exe}\"")
}

// --- Windows startup (HKCU\...\Run) ---

pub fn autostart_enabled() -> bool {
    unsafe {
        let key = wide(RUN_KEY);
        let value = wide(RUN_VALUE);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let mut size = 0u32;
        let present = RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            None,
            None,
            Some(&mut size),
        ) == ERROR_SUCCESS
            && size > 0;
        let _ = RegCloseKey(hkey);
        present
    }
}

pub fn set_autostart(enable: bool) -> bool {
    unsafe {
        let key = wide(RUN_KEY);
        let value = wide(RUN_VALUE);
        if !enable {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                0,
                KEY_WRITE,
                &mut hkey,
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let r = RegDeleteValueW(hkey, PCWSTR(value.as_ptr()));
            let _ = RegCloseKey(hkey);
            // Deleting a missing value is still the desired end state.
            return r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND;
        }
        let mut hkey = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            0,
            PCWSTR::null(),
            Default::default(),
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let exe = match std::env::current_exe() {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(_) => return false,
        };
        // Quote so install locations containing spaces keep launching
        // (ISSUE-002). The NUL terminator is included in the byte count.
        let quoted = quote_run_path(&exe);
        let data: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: &[u8] = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let r = RegSetValueExW(hkey, PCWSTR(value.as_ptr()), 0, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        r == ERROR_SUCCESS
    }
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
    fn run_path_is_quoted() {
        assert_eq!(
            quote_run_path(r"C:\Program Files\App\app.exe"),
            r#""C:\Program Files\App\app.exe""#
        );
        assert_eq!(quote_run_path(r"C:\app.exe"), r#""C:\app.exe""#);
    }

    #[test]
    fn resolve_sound_file_accepts_only_existing_files() {
        assert_eq!(resolve_sound_file(None), None);
        assert_eq!(resolve_sound_file(Some("")), None);
        assert_eq!(resolve_sound_file(Some("   ")), None);
        assert_eq!(
            resolve_sound_file(Some(r"C:\definitely\not\hereibly.wav")),
            None
        );
        let wav = std::env::temp_dir().join(format!("kcfix_sndtest_{}.wav", std::process::id()));
        std::fs::write(&wav, b"RIFF").unwrap();
        assert_eq!(resolve_sound_file(wav.to_str()), Some(wav.clone()));
        let _ = std::fs::remove_file(&wav);
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
            };
            save(&s);
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
        });
    }

    #[test]
    fn torn_main_file_falls_back_to_backup() {
        with_isolated_appdata(|dir| {
            let first = Settings {
                threshold_ms: 111,
                ..Default::default()
            };
            save(&first);
            let second = Settings {
                threshold_ms: 222,
                ..Default::default()
            };
            save(&second);
            assert_eq!(load().threshold_ms, 222);
            // Simulate a crash mid-write: truncated main file.
            let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
            std::fs::write(&ini, "").unwrap();
            // Previous generation (111) is recovered, not defaults (50).
            assert_eq!(load().threshold_ms, 111);
        });
    }
}
