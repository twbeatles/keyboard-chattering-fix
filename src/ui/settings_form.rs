//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use windows::{
    Win32::{
        Foundation::HWND,
        UI::{
            Controls::Dialogs::{
                GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
                OFN_PATHMUSTEXIST, OPENFILENAMEW,
            },
            Controls::EM_SETLIMITTEXT,
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, PWSTR},
};

use super::{
    history,
    ids::*,
    state::{SAVED, current_lang, set_lang},
    text::{apply_language, combo_sel, get_text, refresh_footer, set_text, to_wide},
    widgets::{get_toggle, set_toggle},
};
use crate::{
    app::state::SHARED,
    config::{self, Settings},
    debounce::{LOG_LIMIT_MAX, LOG_LIMIT_MIN, MeasureFrom, RELEASE_DEFER_MAX_MS},
    i18n::{LanguagePref, strings, system_language},
    input::sound::{play_notification_sound, play_path_for_preview},
};

/// Set while the program itself writes into fields, so the resulting
/// `EN_CHANGE` notifications are not mistaken for user edits.
static SUPPRESS_DIRTY: AtomicBool = AtomicBool::new(false);
static DIRTY: AtomicBool = AtomicBool::new(false);
/// Timer ticks left before the "Saved" confirmation disappears.
static SAVED_TICKS: AtomicU32 = AtomicU32::new(0);
/// 500 ms ticks the "Saved" confirmation stays visible.
const SAVED_VISIBLE_TICKS: u32 = 6;

/// Parse `text` as `u64` clamped to `[min, max]`; unparsable input keeps
/// `fallback` (the live value), so a typo never resets a setting.
fn clamp_u64(text: &str, fallback: u64, min: u64, max: u64) -> u64 {
    text.trim()
        .parse::<u64>()
        .map(|n| n.clamp(min, max))
        .unwrap_or(fallback)
}

unsafe fn set_field(hwnd: HWND, id: i32, text: &str) {
    unsafe {
        SUPPRESS_DIRTY.store(true, Ordering::Relaxed);
        set_text(hwnd, id, text);
        SUPPRESS_DIRTY.store(false, Ordering::Relaxed);
    }
}

/// Read a numeric edit, normalize the visible text to the clamped value,
/// and return it.
unsafe fn read_bounded_number(hwnd: HWND, id: i32, fallback: u64, min: u64, max: u64) -> u64 {
    unsafe {
        let value = clamp_u64(&get_text(hwnd, id), fallback, min, max);
        set_field(hwnd, id, &value.to_string());
        value
    }
}

unsafe fn select(hwnd: HWND, id: i32, index: usize) {
    unsafe {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            SendMessageW(
                h,
                CB_SETCURSEL,
                Some(windows::Win32::Foundation::WPARAM(index)),
                Some(windows::Win32::Foundation::LPARAM(0)),
            );
        }
    }
}

/// Show `init` in every field (startup).
pub(crate) unsafe fn load_into_form(hwnd: HWND, init: &Settings) {
    unsafe {
        set_field(hwnd, ID_EDIT_THRESHOLD, &init.threshold_ms.to_string());
        set_field(hwnd, ID_EDIT_MIN_CHATTER, &init.min_chatter_ms.to_string());
        set_field(hwnd, ID_EDIT_LOG_LIMIT, &init.log_limit.to_string());
        set_field(
            hwnd,
            ID_EDIT_SOUND_FILE,
            init.sound_file.as_deref().unwrap_or(""),
        );
        set_field(hwnd, ID_EDIT_DEFER, &init.release_defer_ms.to_string());
        set_field(hwnd, ID_EDIT_AUTO_APPS, &init.auto_disable_apps);
        set_field(hwnd, ID_EDIT_HOTKEY_TOGGLE, &init.hotkey_toggle);
        set_field(hwnd, ID_EDIT_HOTKEY_CLEAN, &init.hotkey_cleaning);
        for id in [
            ID_EDIT_THRESHOLD,
            ID_EDIT_MIN_CHATTER,
            ID_EDIT_LOG_LIMIT,
            ID_EDIT_DEFER,
        ] {
            if let Ok(h) = GetDlgItem(Some(hwnd), id) {
                SendMessageW(
                    h,
                    EM_SETLIMITTEXT,
                    Some(windows::Win32::Foundation::WPARAM(4)),
                    Some(windows::Win32::Foundation::LPARAM(0)),
                );
            }
        }

        set_toggle(hwnd, ID_TOGGLE_ENABLE, init.enabled);
        set_toggle(hwnd, ID_TOGGLE_SOUND, init.play_sound);
        set_toggle(hwnd, ID_TOGGLE_AUTOSTART, config::autostart_enabled());
        set_toggle(hwnd, ID_TOGGLE_MINIMIZE, init.minimize_on_launch);
        set_toggle(hwnd, ID_TOGGLE_CLOSE_TO_TRAY, init.close_to_tray);
        set_toggle(hwnd, ID_TOGGLE_EXCLUDE_INJECTED, init.exclude_injected);
        set_toggle(hwnd, ID_TOGGLE_RECORD_ALL, init.record_all_keys);
        set_toggle(hwnd, ID_TOGGLE_MOUSE, init.filter_mouse);
        set_toggle(
            hwnd,
            ID_TOGGLE_AUTO_FULLSCREEN,
            init.auto_disable_fullscreen,
        );

        select(
            hwnd,
            ID_COMBO_MEASURE,
            if init.measure_from == MeasureFrom::Press {
                1
            } else {
                0
            },
        );
        select(hwnd, ID_COMBO_LANG, init.language.combo_index());
        // What is on disk now: the autosave only writes once this moves.
        *PERSISTED.lock().unwrap() = Some(LiveFingerprint {
            total: init.total_presses,
            blocked: init.blocked_presses,
            enabled: init.enabled,
        });
        DIRTY.store(false, Ordering::Relaxed);
        set_text(hwnd, ID_SAVE_STATUS, "");
    }
}

/// A settings field changed by the user: say it is not saved yet.
pub(crate) unsafe fn mark_dirty(hwnd: HWND) {
    unsafe {
        if SUPPRESS_DIRTY.load(Ordering::Relaxed) {
            return;
        }
        SAVED_TICKS.store(0, Ordering::Relaxed);
        if !DIRTY.swap(true, Ordering::Relaxed) {
            set_text(hwnd, ID_SAVE_STATUS, strings(current_lang()).status_dirty);
        }
    }
}

/// Timer tick: let the "Saved" confirmation fade out.
pub(crate) unsafe fn tick(hwnd: HWND) {
    unsafe {
        let left = SAVED_TICKS.load(Ordering::Relaxed);
        if left == 0 {
            return;
        }
        SAVED_TICKS.store(left - 1, Ordering::Relaxed);
        if left == 1 && !DIRTY.load(Ordering::Relaxed) {
            set_text(hwnd, ID_SAVE_STATUS, "");
        }
    }
}

/// Retranslate the status line after a language switch.
pub(crate) unsafe fn refresh_status(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let text = if DIRTY.load(Ordering::Relaxed) {
            s.status_dirty
        } else if SAVED_TICKS.load(Ordering::Relaxed) > 0 {
            s.status_saved
        } else {
            ""
        };
        set_text(hwnd, ID_SAVE_STATUS, text);
    }
}

/// What `persist_live` last wrote (or what was loaded), so the periodic
/// autosave only touches the disk when something actually changed.
static PERSISTED: Mutex<Option<LiveFingerprint>> = Mutex::new(None);
/// 500 ms ticks since the last autosave check.
static AUTOSAVE_TICKS: AtomicU32 = AtomicU32::new(0);
/// Autosave check interval: 5 minutes of 500 ms ticks (ISSUE-004).
const AUTOSAVE_INTERVAL_TICKS: u32 = 600;
/// Bumped per settings save; a sound validation finishing late only applies
/// when it still belongs to the newest save.
static SOUND_GEN: AtomicU64 = AtomicU64::new(0);

/// Live state that `persist_live` writes: counters and protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveFingerprint {
    total: u64,
    blocked: u64,
    enabled: bool,
}

fn live_fingerprint() -> Option<LiveFingerprint> {
    SHARED.get().map(|shared| {
        let g = shared.lock().unwrap();
        LiveFingerprint {
            total: g.debouncer.total_presses,
            blocked: g.debouncer.blocked_presses,
            enabled: g.enabled,
        }
    })
}

/// Whether the periodic autosave should write: only on interval ticks and
/// only when the live state moved since the last write.
fn autosave_due(
    ticks: u32,
    persisted: Option<LiveFingerprint>,
    live: Option<LiveFingerprint>,
) -> bool {
    ticks >= AUTOSAVE_INTERVAL_TICKS && live.is_some() && live != persisted
}

/// Write the last saved settings plus live counters and protection state.
/// Used on exit, session end, the periodic autosave, after a statistics
/// reset, and after a language switch.
///
/// File-only fields (`key.XX`) are re-read from disk first, so edits made in
/// the settings file while the app runs survive (ISSUE-005).
pub(crate) unsafe fn persist_live() -> bool {
    persist_with(|_| {})
}

/// [`persist_live`] with one immediate change applied on top of the file's
/// current per-key values (the Keys tab: per-key windows, mouse filter).
fn persist_with(change: impl FnOnce(&mut Settings)) -> bool {
    let Some(saved) = SAVED.get() else {
        return false;
    };
    let disk = config::try_load();
    let mut s = saved.lock().unwrap();
    if let Some(disk) = &disk {
        s.adopt_file_only_fields(disk);
    }
    change(&mut s);
    let live = SHARED.get().map(|shared| {
        let g = shared.lock().unwrap();
        s.enabled = g.enabled;
        s.total_presses = g.debouncer.total_presses;
        s.blocked_presses = g.debouncer.blocked_presses;
        s.per_key_presses = g.debouncer.per_key_presses;
        s.per_key_blocked = g.debouncer.per_key_blocked;
        LiveFingerprint {
            total: g.debouncer.total_presses,
            blocked: g.debouncer.blocked_presses,
            enabled: g.enabled,
        }
    });
    let written = config::save(&s);
    if written {
        *PERSISTED.lock().unwrap() = live;
    }
    written
}

/// Set (`Some`) or clear (`None`) one key's block window, apply it to the
/// live filter, and write it to the settings file right away. The rest of
/// the file's per-key values are re-read first, so a concurrent edit of the
/// file is kept. Returns whether the change reached disk.
pub(crate) fn set_key_override(vk: u8, ms: Option<u64>) -> bool {
    let ms = ms.map(|m| m.clamp(1, 1000));
    let mut per_key = None;
    let written = persist_with(|s| {
        s.per_key_ms[vk as usize] = ms;
        per_key = Some(s.per_key_ms);
    });
    if let (Some(per_key), Some(shared)) = (per_key, SHARED.get()) {
        let mut g = shared.lock().unwrap();
        for (k, ms) in per_key.iter().enumerate() {
            g.debouncer.set_key_threshold(k as u8, *ms);
        }
    }
    history::invalidate();
    written
}

/// Mouse filter switch: applies at once (installs or removes the mouse
/// hook) and is saved right away, like the protection switch.
pub(crate) fn set_mouse_filter(on: bool) -> bool {
    crate::input::hook::set_mouse_filter(on);
    persist_with(|s| s.filter_mouse = on)
}

/// Timer tick: persist counters every few minutes when they changed, so a
/// crash or a forced shutdown loses at most one interval (ISSUE-004).
pub(crate) unsafe fn autosave_tick() {
    unsafe {
        let ticks = AUTOSAVE_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        if ticks < AUTOSAVE_INTERVAL_TICKS {
            return;
        }
        AUTOSAVE_TICKS.store(0, Ordering::Relaxed);
        let persisted = *PERSISTED.lock().unwrap();
        if autosave_due(ticks, persisted, live_fingerprint()) {
            persist_live();
        }
    }
}

/// Validate the sound path on a worker thread (it touches the disk and may
/// sit on an unreachable network share) and apply it when done (G7).
fn apply_sound_async(path: Option<String>) {
    let generation = SOUND_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let validated = config::resolve_sound_file(path.as_deref());
        if let Some(shared) = SHARED.get() {
            let mut g = shared.lock().unwrap();
            // Checked under the lock: an older, slower validation can never
            // overwrite the result of a newer save.
            if SOUND_GEN.load(Ordering::SeqCst) == generation {
                g.sound_file = validated;
            }
        }
    });
}

/// Language combo: switch the UI right away and remember the choice.
pub(crate) unsafe fn on_language_changed(hwnd: HWND) {
    unsafe {
        // Row 0 is "Automatic" (follow Windows); the rest are fixed languages.
        let pref = LanguagePref::from_combo_index(combo_sel(hwnd, ID_COMBO_LANG));
        if let Some(saved) = SAVED.get() {
            saved.lock().unwrap().language = pref;
        }
        let lang = pref.resolve(system_language());
        if lang == current_lang() {
            // Only the stored choice changed (e.g. Automatic -> the language it
            // already resolves to): remember it, nothing to retranslate.
            persist_live();
            return;
        }
        set_lang(lang);
        persist_live();
        apply_language(hwnd);
        refresh_status(hwnd);
    }
}

pub(crate) unsafe fn preview_sound(hwnd: HWND) {
    let path = get_text(hwnd, ID_EDIT_SOUND_FILE);
    if path.is_empty() {
        // Blank means "system sound": preview exactly that.
        play_notification_sound(None);
    } else {
        play_path_for_preview(std::path::Path::new(&path));
    }
}

pub(crate) unsafe fn browse_sound(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let mut file = [0u16; 1024];
        let current = to_wide(&get_text(hwnd, ID_EDIT_SOUND_FILE));
        let n = current.len().min(file.len() - 1);
        file[..n].copy_from_slice(&current[..n]);
        // "Name\0*.wav\0\0"
        let filter: Vec<u16> = format!("{}\0*.wav\0", s.sound_filter)
            .encode_utf16()
            .chain([0])
            .collect();
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(file.as_mut_ptr()),
            nMaxFile: file.len() as u32,
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
            let picked = String::from_utf16_lossy(&file[..len]);
            set_text(hwnd, ID_EDIT_SOUND_FILE, &picked);
            mark_dirty(hwnd);
        }
    }
}

pub(crate) unsafe fn on_save(hwnd: HWND) {
    unsafe {
        // Live fallbacks so a missing control or a typo keeps the running value.
        let (threshold_fb, min_chatter_fb, log_limit_fb) = SHARED
            .get()
            .map(|g| {
                let g = g.lock().unwrap();
                (
                    g.debouncer.threshold_ms(),
                    g.debouncer.min_chatter_ms(),
                    g.debouncer.log_limit() as u64,
                )
            })
            .unwrap_or((50, 0, 100));

        let threshold = read_bounded_number(hwnd, ID_EDIT_THRESHOLD, threshold_fb, 1, 1000);
        let min_chatter = read_bounded_number(hwnd, ID_EDIT_MIN_CHATTER, min_chatter_fb, 0, 1000);
        let log_limit = read_bounded_number(
            hwnd,
            ID_EDIT_LOG_LIMIT,
            log_limit_fb,
            LOG_LIMIT_MIN as u64,
            LOG_LIMIT_MAX as u64,
        ) as usize;
        let sound_file = Some(get_text(hwnd, ID_EDIT_SOUND_FILE)).filter(|s| !s.is_empty());
        let defer_fb = SHARED
            .get()
            .map_or(0, |g| g.lock().unwrap().debouncer.release_defer_ms());
        let release_defer =
            read_bounded_number(hwnd, ID_EDIT_DEFER, defer_fb, 0, RELEASE_DEFER_MAX_MS);
        let auto_apps = get_text(hwnd, ID_EDIT_AUTO_APPS);
        let auto_fullscreen = get_toggle(ID_TOGGLE_AUTO_FULLSCREEN);
        let hotkey_toggle = get_text(hwnd, ID_EDIT_HOTKEY_TOGGLE);
        let hotkey_cleaning = get_text(hwnd, ID_EDIT_HOTKEY_CLEAN);

        let enabled = get_toggle(ID_TOGGLE_ENABLE);
        let sound = get_toggle(ID_TOGGLE_SOUND);
        let autostart = get_toggle(ID_TOGGLE_AUTOSTART);
        let minimize = get_toggle(ID_TOGGLE_MINIMIZE);
        let close_to_tray = get_toggle(ID_TOGGLE_CLOSE_TO_TRAY);
        let exclude_injected = get_toggle(ID_TOGGLE_EXCLUDE_INJECTED);
        let record_all = get_toggle(ID_TOGGLE_RECORD_ALL);
        let measure = if combo_sel(hwnd, ID_COMBO_MEASURE) == 1 {
            MeasureFrom::Press
        } else {
            MeasureFrom::Release
        };

        let s = strings(current_lang());
        if !config::set_autostart(autostart) {
            // Show the real state instead of a toggle that did not apply (G2).
            set_toggle(hwnd, ID_TOGGLE_AUTOSTART, config::autostart_enabled());
            super::dialog::msg_box(hwnd, s.title, s.autostart_failed, MB_OK | MB_ICONWARNING);
        }
        // Disk is the base so file-only fields (`key.XX`) edited while running
        // are kept and, below, applied to the live filter (ISSUE-005, G3).
        let mut new_settings = config::load();
        new_settings.threshold_ms = threshold;
        new_settings.enabled = enabled;
        new_settings.play_sound = sound;
        new_settings.sound_file = sound_file.clone();
        new_settings.minimize_on_launch = minimize;
        new_settings.close_to_tray = close_to_tray;
        new_settings.record_all_keys = record_all;
        new_settings.measure_from = measure;
        new_settings.language = LanguagePref::from_combo_index(combo_sel(hwnd, ID_COMBO_LANG));
        new_settings.min_chatter_ms = min_chatter;
        new_settings.exclude_injected = exclude_injected;
        new_settings.filter_mouse = get_toggle(ID_TOGGLE_MOUSE);
        new_settings.release_defer_ms = release_defer;
        new_settings.auto_disable_apps = auto_apps.clone();
        new_settings.auto_disable_fullscreen = auto_fullscreen;
        new_settings.hotkey_toggle = hotkey_toggle.clone();
        new_settings.hotkey_cleaning = hotkey_cleaning.clone();
        new_settings.log_limit = log_limit;

        let live = SHARED.get().map(|g| {
            let mut g = g.lock().unwrap();
            g.debouncer.set_threshold_ms(threshold);
            for (vk, ms) in new_settings.per_key_ms.iter().enumerate() {
                g.debouncer.set_key_threshold(vk as u8, *ms);
            }
            g.debouncer.set_measure_from(measure);
            g.debouncer.set_min_chatter_ms(min_chatter);
            g.debouncer.set_release_defer_ms(release_defer);
            g.debouncer.record_all_keys = record_all;
            g.debouncer.set_log_limit(log_limit);
            g.enabled = enabled;
            g.play_sound = sound;
            g.exclude_injected = exclude_injected;
            new_settings.total_presses = g.debouncer.total_presses;
            new_settings.blocked_presses = g.debouncer.blocked_presses;
            new_settings.per_key_presses = g.debouncer.per_key_presses;
            new_settings.per_key_blocked = g.debouncer.per_key_blocked;
            LiveFingerprint {
                total: g.debouncer.total_presses,
                blocked: g.debouncer.blocked_presses,
                enabled: g.enabled,
            }
        });
        // The hook must never touch the filesystem: validated off-thread, then
        // swapped in under the lock.
        apply_sound_async(sound_file.clone());
        crate::input::hook::set_mouse_filter(new_settings.filter_mouse);
        super::foreground::configure(&auto_apps, auto_fullscreen);
        let hotkey_problems = super::hotkeys::apply(hwnd, &hotkey_toggle, &hotkey_cleaning);
        super::menu::install(hwnd);
        let written = config::save(&new_settings);
        if written {
            *PERSISTED.lock().unwrap() = live;
        }
        if let Some(saved) = SAVED.get() {
            *saved.lock().unwrap() = new_settings;
        }
        DIRTY.store(false, Ordering::Relaxed);
        if written {
            SAVED_TICKS.store(SAVED_VISIBLE_TICKS, Ordering::Relaxed);
            set_text(hwnd, ID_SAVE_STATUS, s.status_saved);
        } else {
            // The form still shows the applied in-memory values, so tell the
            // user they did not reach disk (ISSUE-004).
            set_text(hwnd, ID_SAVE_STATUS, "");
            super::dialog::msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
        refresh_footer(hwnd, close_to_tray);
        history::invalidate();
        super::keys_page::invalidate();
        super::stats::update_stats(hwnd);
        if !hotkey_problems.is_empty() {
            super::dialog::msg_box(
                hwnd,
                s.title,
                &hotkey_problems.join("\n\n"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_accepts_value_inside_range() {
        assert_eq!(clamp_u64("80", 50, 1, 1000), 80);
        assert_eq!(clamp_u64("  80  ", 50, 1, 1000), 80);
    }

    #[test]
    fn clamp_pins_to_bounds() {
        assert_eq!(clamp_u64("0", 50, 1, 1000), 1);
        assert_eq!(clamp_u64("5000", 50, 1, 1000), 1000);
        // Zero is a valid minimum for the min-chatter gap (disables it).
        assert_eq!(clamp_u64("0", 9, 0, 1000), 0);
    }

    fn fp(total: u64, blocked: u64, enabled: bool) -> Option<LiveFingerprint> {
        Some(LiveFingerprint {
            total,
            blocked,
            enabled,
        })
    }

    #[test]
    fn autosave_waits_for_the_interval() {
        let (old, new) = (fp(1, 0, true), fp(5, 1, true));
        assert!(!autosave_due(AUTOSAVE_INTERVAL_TICKS - 1, old, new));
        assert!(autosave_due(AUTOSAVE_INTERVAL_TICKS, old, new));
    }

    #[test]
    fn autosave_skips_when_nothing_changed() {
        let same = fp(5, 1, true);
        assert!(!autosave_due(AUTOSAVE_INTERVAL_TICKS, same, same));
        // Toggling protection alone is worth persisting.
        assert!(autosave_due(AUTOSAVE_INTERVAL_TICKS, same, fp(5, 1, false)));
        // No live state (filter not initialized): nothing to write.
        assert!(!autosave_due(AUTOSAVE_INTERVAL_TICKS, same, None));
        // Never persisted yet: the first live state is written.
        assert!(autosave_due(AUTOSAVE_INTERVAL_TICKS, None, same));
    }

    #[test]
    fn clamp_keeps_fallback_on_garbage() {
        assert_eq!(clamp_u64("", 50, 1, 1000), 50);
        assert_eq!(clamp_u64("abc", 50, 1, 1000), 50);
        assert_eq!(clamp_u64("-3", 50, 1, 1000), 50);
    }
}
