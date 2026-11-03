//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use super::super::model::Settings;
use crate::{
    debounce::{GAP_BUCKETS, MeasureFrom, RELEASE_DEFER_MAX_MS},
    i18n::LanguagePref,
};

use super::guard::looks_like_settings;
use super::io::{load, load_from, save, try_load};
use super::parse::{CLEANING_TIMEOUT_MAX_MIN, parse};
use crate::i18n::Language;

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
    unsafe {
        std::env::set_var("APPDATA", &dir);
    }
    f(&dir);
    match previous {
        Some(v) => unsafe { std::env::set_var("APPDATA", v) },
        None => unsafe { std::env::remove_var("APPDATA") },
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `ini` sample blocks of the README (English and Korean).
fn readme_samples() -> Vec<String> {
    include_str!("../../../README.md")
        .replace("\r\n", "\n")
        .split("```ini\n")
        .skip(1)
        .filter_map(|rest| rest.split("```").next())
        .filter(|block| block.starts_with("[KeyboardChatteringFix]"))
        .map(str::to_string)
        .collect()
}

/// The README shows the settings file with trailing `#` comments; a
/// file written like it must mean what the comments say (`0` is off),
/// not "any text but 0 is on" (2026-10-22 audit ISSUE-001).
#[test]
fn readme_sample_parses_to_what_it_documents() {
    let samples = readme_samples();
    assert_eq!(samples.len(), 2, "English and Korean sample");
    for sample in &samples {
        let s = parse(sample);
        let d = Settings::default();
        assert_eq!(s.threshold_ms, d.threshold_ms);
        assert!(s.enabled);
        assert!(!s.play_sound, "bPlaySound=0 with a comment is off");
        assert_eq!(s.sound_file, None);
        assert!(!s.minimize_on_launch);
        assert!(s.close_to_tray);
        assert!(!s.filter_mouse, "bFilterMouse=0 with a comment is off");
        assert!(!s.auto_disable_fullscreen);
        assert_eq!(s.language, LanguagePref::Auto);
        assert_eq!(s.measure_from, MeasureFrom::Release);
        assert_eq!(s.log_limit, d.log_limit);
        assert_eq!(s.cleaning_timeout_min, 5);
        assert_eq!(s.auto_disable_apps, "");
        assert_eq!(
            (s.hotkey_toggle.as_str(), s.hotkey_cleaning.as_str()),
            ("", "")
        );
        assert_eq!(s.per_key_ms[0x48], Some(120));
        assert_eq!(s.per_key_ms[0x20], Some(80));
        assert_eq!(s.per_key_ms[0x01], Some(30));
        assert_eq!(s.per_key_ms[0x02], Some(0), "key.02=0: never filtered");
        assert_eq!(s.per_key_ms.iter().flatten().count(), 4);
        assert_eq!(s.mouse_threshold_ms, 30);
    }
}

#[test]
fn trailing_comments_need_whitespace_and_keep_paths_and_app_lists() {
    let s = parse(
        "[KeyboardChatteringFix]\n\
             iThreshold=80 ; eighty\n\
             lpSoundFile=C:\\My #1\\blip.wav\n\
             sAutoDisableApps=a.exe ; b.exe # games\n\
             sHotkeyToggle=Ctrl+Alt+F12\t# toggle\n",
    );
    assert_eq!(s.threshold_ms, 80);
    assert_eq!(s.sound_file.as_deref(), Some(r"C:\My #1\blip.wav"));
    assert_eq!(s.auto_disable_apps, "a.exe ; b.exe");
    assert_eq!(s.hotkey_toggle, "Ctrl+Alt+F12");
}

/// A main file torn by a power loss (right size, all zeros) or holding
/// something else must fall back to the backup instead of loading as
/// "everything at its default" (2026-10-22 audit ISSUE-002).
#[test]
fn unrecognizable_main_file_falls_back_to_the_backup() {
    with_isolated_appdata(|dir| {
        let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
        let bak = ini.with_extension("ini.bak");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        std::fs::write(&bak, "[KeyboardChatteringFix]\niThreshold=111\nkey.41=90\n").unwrap();
        for torn in [vec![0u8; 512], b"hello\n".to_vec()] {
            std::fs::write(&ini, &torn).unwrap();
            let s = try_load().expect("backup");
            assert_eq!(s.threshold_ms, 111);
            assert_eq!(s.per_key_ms[0x41], Some(90));
        }
        // Neither readable: nothing, not the defaults.
        std::fs::write(&bak, [0u8; 64]).unwrap();
        assert!(try_load().is_none());
    });
}

/// Saving over a torn main file keeps the good backup instead of
/// copying the torn file over it.
#[test]
fn save_never_backs_up_a_torn_main_file() {
    with_isolated_appdata(|dir| {
        let with_threshold = |threshold_ms| Settings {
            threshold_ms,
            ..Settings::default()
        };
        assert!(save(&with_threshold(111)));
        assert!(save(&with_threshold(222)));
        let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
        let bak = ini.with_extension("ini.bak");
        assert!(
            std::fs::read_to_string(&bak)
                .unwrap()
                .contains("iThreshold=111")
        );

        std::fs::write(&ini, [0u8; 512]).unwrap();
        assert!(save(&with_threshold(333)));
        assert!(
            std::fs::read_to_string(&bak)
                .unwrap()
                .contains("iThreshold=111")
        );
        assert_eq!(load().threshold_ms, 333);
        assert!(!ini.with_extension("ini.tmp").exists());
    });
}

/// A hand-edited file saved in the system code page (not UTF-8) is
/// read, not silently replaced by the backup.
#[test]
fn non_utf8_file_is_read_in_the_system_code_page() {
    with_isolated_appdata(|dir| {
        let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        let mut bytes = b"[KeyboardChatteringFix]\niThreshold=77\nlpSoundFile=C:\\s".to_vec();
        // "가" in CP949, "°¡" in CP1252: whole characters either way.
        bytes.extend_from_slice(&[0xB0, 0xA1]);
        bytes.extend_from_slice(b".wav\n");
        assert!(std::str::from_utf8(&bytes).is_err());
        std::fs::write(&ini, &bytes).unwrap();
        let s = try_load().expect("decoded");
        assert_eq!(s.threshold_ms, 77);
        assert!(s.sound_file.unwrap().ends_with(".wav"));
        // A UTF-8 BOM is not part of the header line.
        std::fs::write(
            &ini,
            b"\xEF\xBB\xBF[KeyboardChatteringFix]\niThreshold=66\n",
        )
        .unwrap();
        assert_eq!(load().threshold_ms, 66);
    });
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
        assert_eq!(s.language, LanguagePref::Fixed(Language::En));
        assert_eq!(s.measure_from, MeasureFrom::Press);
        assert_eq!(s.min_chatter_ms, 1000);
        assert!(!s.exclude_injected);
        assert_eq!((s.total_presses, s.blocked_presses), (7, 2));
        assert_eq!(s.per_key_ms[0x41], Some(200));
    });
}

#[test]
fn language_defaults_to_automatic_and_round_trips() {
    with_isolated_appdata(|dir| {
        let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        // A settings file without the key (fresh install, hand-made file)
        // follows the Windows language.
        std::fs::write(
            &ini,
            "[KeyboardChatteringFix]
iThreshold=60
",
        )
        .unwrap();
        assert_eq!(load().language, LanguagePref::Auto);
        // No file at all is the same.
        std::fs::remove_file(&ini).unwrap();
        assert_eq!(load().language, LanguagePref::Auto);

        for (text, want) in [
            ("auto", LanguagePref::Auto),
            ("ko", LanguagePref::Fixed(Language::Ko)),
            ("en", LanguagePref::Fixed(Language::En)),
            // v0.2.x wrote the code of the resolved language; still honoured.
            ("KO", LanguagePref::Fixed(Language::Ko)),
            ("klingon", LanguagePref::Auto),
        ] {
            std::fs::write(
                &ini,
                format!(
                    "[KeyboardChatteringFix]
sLanguage={text}
"
                ),
            )
            .unwrap();
            assert_eq!(load().language, want, "{text}");
        }

        for pref in [LanguagePref::Auto, LanguagePref::Fixed(Language::Ko)] {
            let s = Settings {
                language: pref,
                ..Default::default()
            };
            assert!(save(&s));
            assert_eq!(load().language, pref);
        }
        let saved = std::fs::read_to_string(&ini).unwrap();
        assert!(
            saved.contains(
                "sLanguage=ko
"
            ),
            "{saved}"
        );
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
            language: LanguagePref::Fixed(Language::En),
            per_key_ms,
            mouse_threshold_ms: 45,
            measure_from: MeasureFrom::Press,
            min_chatter_ms: 9,
            exclude_injected: false,
            filter_mouse: true,
            release_defer_ms: 30,
            auto_disable_apps: "game.exe; other".to_string(),
            auto_disable_fullscreen: true,
            hotkey_toggle: "Ctrl+Alt+F12".to_string(),
            hotkey_cleaning: "Ctrl+Alt+F11".to_string(),
            cleaning_timeout_min: 0,
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
            gap_hist: {
                let mut hist = crate::config::model::empty_histograms();
                hist[0x20][2] = 9;
                hist
            },
        };
        assert!(save(&s));
        // No staging file may be left behind by the atomic write.
        assert!(
            !dir.join("KeyboardChatteringFix")
                .join("settings.ini.tmp")
                .exists()
        );
        let back = load();
        assert_eq!(back.threshold_ms, 77);
        assert!(!back.enabled);
        assert!(back.play_sound);
        assert_eq!(back.sound_file.as_deref(), Some(r"D:\audio\blip.wav"));
        assert!(back.minimize_on_launch);
        assert!(!back.close_to_tray);
        assert_eq!(back.language, LanguagePref::Fixed(Language::En));
        assert_eq!(back.per_key_ms[0x20], Some(80));
        assert_eq!(back.measure_from, MeasureFrom::Press);
        assert_eq!(back.min_chatter_ms, 9);
        assert!(!back.exclude_injected);
        assert!(back.filter_mouse);
        assert_eq!(back.mouse_threshold_ms, 45);
        assert_eq!(back.gap_hist[0x20][2], 9);
        assert_eq!(back.release_defer_ms, 30);
        assert_eq!(back.auto_disable_apps, "game.exe; other");
        assert!(back.auto_disable_fullscreen);
        assert_eq!(back.hotkey_toggle, "Ctrl+Alt+F12");
        assert_eq!(back.hotkey_cleaning, "Ctrl+Alt+F11");
        assert_eq!(back.cleaning_timeout_min, 0);
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
    let blocker = std::env::temp_dir().join(format!("kcfix_cfgblock_{}_{seq}", std::process::id()));
    std::fs::write(&blocker, b"not-a-directory").unwrap();
    unsafe {
        std::env::set_var("APPDATA", &blocker);
    }
    assert!(!save(&Settings::default()));
    match previous {
        Some(v) => unsafe { std::env::set_var("APPDATA", v) },
        None => unsafe { std::env::remove_var("APPDATA") },
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
                "[KeyboardChatteringFix]\nbPlaySound=2\nbRecordAllKeypress=yes\nbFilterEnabled=0\nbMinimizeOnLaunch=yes\n",
            )
            .unwrap();
        let s = load();
        assert!(s.play_sound);
        assert!(s.record_all_keys);
        assert!(!s.enabled);
        assert!(s.minimize_on_launch);
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
fn new_fields_are_clamped_and_kept_on_one_line() {
    with_isolated_appdata(|dir| {
        let ini = dir.join("KeyboardChatteringFix").join("settings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        std::fs::write(
            &ini,
            "[KeyboardChatteringFix]\niReleaseDeferMs=900\niCleaningTimeoutMin=9999\n",
        )
        .unwrap();
        let mut s = load();
        assert_eq!(s.release_defer_ms, RELEASE_DEFER_MAX_MS);
        assert_eq!(s.cleaning_timeout_min, CLEANING_TIMEOUT_MAX_MIN);
        // A pasted line break must not inject another key.
        s.auto_disable_apps = "a.exe\niThreshold=1".to_string();
        assert!(save(&s));
        let back = load();
        assert_eq!(back.auto_disable_apps, "a.exe iThreshold=1");
        assert_eq!(back.threshold_ms, s.threshold_ms);
        // Defaults when absent.
        assert_eq!(Settings::default().cleaning_timeout_min, 5);
    });
}

#[test]
fn import_accepts_only_settings_files() {
    with_isolated_appdata(|dir| {
        std::fs::create_dir_all(dir).unwrap();
        let good = dir.join("good.ini");
        std::fs::write(&good, "[KeyboardChatteringFix]\niThreshold=77\n").unwrap();
        assert_eq!(load_from(&good).unwrap().threshold_ms, 77);
        let bad = dir.join("bad.txt");
        std::fs::write(&bad, "hello\n").unwrap();
        assert!(load_from(&bad).is_none());
        assert!(load_from(&dir.join("missing.ini")).is_none());
    });
}

/// Measured gaps survive a restart (the suggestion needs rare chatter
/// to add up), only non-zero buckets of used keys are written, and they
/// come last, after a comment.
#[test]
fn measured_gaps_round_trip_compactly() {
    with_isolated_appdata(|dir| {
        let mut s = Settings::default();
        s.gap_hist[0x41][0] = 3;
        s.gap_hist[0x41][40] = 312;
        s.gap_hist[0x20][16] = 20;
        assert!(save(&s));
        let text = std::fs::read_to_string(dir.join("KeyboardChatteringFix").join("settings.ini"))
            .unwrap();
        let tail: Vec<&str> = text.lines().rev().take(4).collect();
        assert_eq!(
            tail,
            [
                "hist.41=0:3,40:312",
                "hist.20=16:20",
                "sHistMeasure=release",
                "; Measured key gaps for the threshold suggestion (written by the program)",
            ]
        );
        let back = load();
        assert_eq!(back.gap_hist[0x41], s.gap_hist[0x41]);
        assert_eq!(back.gap_hist[0x20], s.gap_hist[0x20]);
        assert_eq!(back.gap_hist[0x42], [0; GAP_BUCKETS]);
        // Nothing measured: no histogram block at all.
        assert!(save(&Settings::default()));
        let text = std::fs::read_to_string(dir.join("KeyboardChatteringFix").join("settings.ini"))
            .unwrap();
        assert!(!text.contains("hist") && !text.contains("sHistMeasure"));
    });
}

#[test]
fn malformed_gap_entries_are_skipped_one_by_one() {
    let s =
        parse("[KeyboardChatteringFix]\nhist.41=0:3,99:5,x:1,2:y,3:4,7\nhist.GG=1:1\nhist.42=\n");
    let mut want = [0; GAP_BUCKETS];
    want[0] = 3;
    want[3] = 4;
    assert_eq!(s.gap_hist[0x41], want);
    assert_eq!(s.gap_hist[0x42], [0; GAP_BUCKETS]);
}

/// Gaps measured from the release mean something else once the baseline
/// was switched to the press by hand: they are dropped, not mixed.
#[test]
fn gaps_from_another_baseline_are_dropped() {
    let other =
        parse("[KeyboardChatteringFix]\nsMeasureFrom=press\nsHistMeasure=release\nhist.41=1:5\n");
    assert_eq!(other.gap_hist[0x41], [0; GAP_BUCKETS]);
    let same =
        parse("[KeyboardChatteringFix]\nsMeasureFrom=press\nsHistMeasure=press\nhist.41=1:5\n");
    assert_eq!(same.gap_hist[0x41][1], 5);
    // Written by hand without the tag: taken as the file's baseline.
    let untagged = parse("[KeyboardChatteringFix]\nhist.41=1:5\n");
    assert_eq!(untagged.gap_hist[0x41][1], 5);
}

#[test]
fn a_file_of_measured_gaps_alone_still_counts_as_settings() {
    assert!(looks_like_settings("hist.41=1:5\n"));
    assert!(looks_like_settings("iMouseThreshold=30\n"));
    assert!(!looks_like_settings("history=1\n"));
}

/// `key.XX=0` means "never filter this key"; the mouse window is clamped
/// like the global one and defaults when absent.
#[test]
fn zero_per_key_window_and_mouse_window_parse() {
    let s = parse("[KeyboardChatteringFix]\nkey.02=0\nkey.41=5000\niMouseThreshold=0\n");
    assert_eq!(s.per_key_ms[0x02], Some(0));
    assert_eq!(s.per_key_ms[0x41], Some(1000));
    assert_eq!(s.mouse_threshold_ms, 1);
    assert_eq!(
        parse("[KeyboardChatteringFix]\n").mouse_threshold_ms,
        crate::debounce::MOUSE_THRESHOLD_DEFAULT_MS
    );
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
