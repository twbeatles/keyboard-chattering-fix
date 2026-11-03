//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use std::sync::atomic::Ordering;

use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

use super::super::{
    history,
    ids::*,
    state::{SAVED, current_lang},
    text::{combo_sel, get_text, refresh_footer, set_text},
    widgets::{get_toggle, set_toggle},
};
use super::fields::{DIRTY, SAVED_TICKS, SAVED_VISIBLE_TICKS, read_bounded_number, set_field};
use super::persist::{LiveFingerprint, PERSISTED, baseline_changed, save_base};
use super::sound::apply_sound_async;
use crate::{
    app::state::SHARED,
    config,
    debounce::{LOG_LIMIT_MAX, LOG_LIMIT_MIN, MeasureFrom, RELEASE_DEFER_MAX_MS},
    i18n::{LanguagePref, strings},
};

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
        if !config::sync_autostart(autostart) {
            // Show the real state instead of a toggle that did not apply (G2).
            set_toggle(hwnd, ID_TOGGLE_AUTOSTART, config::autostart_enabled());
            super::super::dialog::msg_box(
                hwnd,
                s.title,
                s.autostart_failed,
                MB_OK | MB_ICONWARNING,
            );
        }
        // Disk is the base so file-only fields (`key.XX`) edited while running
        // are kept and, below, applied to the live filter (ISSUE-005, G3).
        let saved_now = SAVED.get().map(|s| s.lock().unwrap().clone());
        let mut new_settings = save_base(config::try_load(), saved_now.unwrap_or_default());
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
            g.debouncer
                .set_mouse_threshold_ms(new_settings.mouse_threshold_ms);
            if baseline_changed(g.debouncer.measure_from(), measure) {
                // Gaps from the old baseline would skew every suggestion.
                g.debouncer.clear_gap_histograms();
            }
            g.debouncer.set_measure_from(measure);
            g.debouncer.set_min_chatter_ms(min_chatter);
            g.debouncer.set_release_defer_ms(release_defer);
            g.debouncer.record_all_keys = record_all;
            g.debouncer.set_log_limit(log_limit);
            g.enabled = enabled;
            g.play_sound = sound;
            g.exclude_injected = exclude_injected;
            new_settings.take_live_stats(&g.debouncer);
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
        // A value edited in the file is applied by Save, like `key.XX`.
        set_field(
            hwnd,
            ID_MOUSE_MS_EDIT,
            &new_settings.mouse_threshold_ms.to_string(),
        );
        super::super::foreground::configure(&auto_apps, auto_fullscreen);
        let hotkey_problems = super::super::hotkeys::apply(hwnd, &hotkey_toggle, &hotkey_cleaning);
        super::super::menu::install(hwnd);
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
            super::super::dialog::msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
        refresh_footer(hwnd, close_to_tray);
        history::invalidate();
        super::super::keys_page::invalidate();
        super::super::stats::update_stats(hwnd);
        if !hotkey_problems.is_empty() {
            super::super::dialog::msg_box(
                hwnd,
                s.title,
                &hotkey_problems.join("\n\n"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}
