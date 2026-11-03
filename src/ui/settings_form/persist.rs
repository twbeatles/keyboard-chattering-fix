//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use std::sync::{
    Mutex,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

use super::super::history;
use super::super::state::SAVED;
use crate::{
    app::state::SHARED,
    config::{self, Settings},
    debounce::MeasureFrom,
};

/// What `persist_live` last wrote (or what was loaded), so the periodic
/// autosave only touches the disk when something actually changed.
pub(super) static PERSISTED: Mutex<Option<LiveFingerprint>> = Mutex::new(None);
/// 500 ms ticks since the last autosave check.
pub(super) static AUTOSAVE_TICKS: AtomicU32 = AtomicU32::new(0);
/// Autosave check interval: 5 minutes of 500 ms ticks (ISSUE-004).
pub(super) const AUTOSAVE_INTERVAL_TICKS: u32 = 600;
/// Bumped per settings save; a sound validation finishing late only applies
/// when it still belongs to the newest save.
pub(super) static SOUND_GEN: AtomicU64 = AtomicU64::new(0);

/// Live state that `persist_live` writes: counters and protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LiveFingerprint {
    pub(super) total: u64,
    pub(super) blocked: u64,
    pub(super) enabled: bool,
}

pub(super) fn live_fingerprint() -> Option<LiveFingerprint> {
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
pub(super) fn autosave_due(
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
pub(super) fn persist_with(change: impl FnOnce(&mut Settings)) -> bool {
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
        s.take_live_stats(&g.debouncer);
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

/// Set (`Some`, 0 = never filter) or clear (`None`) one key's block window,
/// apply it to the live filter, and write it to the settings file right
/// away. The rest of the file's per-key values are re-read first, so a
/// concurrent edit of the file is kept. Returns whether the change reached
/// disk.
pub(crate) fn set_key_override(vk: u8, ms: Option<u64>) -> bool {
    set_key_overrides(&[(vk, ms)])
}

/// [`set_key_override`] for several keys with one write ("Apply
/// suggestions").
pub(crate) fn set_key_overrides(changes: &[(u8, Option<u64>)]) -> bool {
    let mut per_key = None;
    let written = persist_with(|s| {
        for &(vk, ms) in changes {
            s.per_key_ms[vk as usize] = ms.map(|m| m.min(1000));
        }
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

/// Whether a Save switches the gap baseline, which voids the gaps
/// measured so far (they mean something else under the other one).
pub(super) fn baseline_changed(live: MeasureFrom, saved: MeasureFrom) -> bool {
    live != saved
}

/// What a Save starts from: the file as it is now, so per-key values edited
/// in it are kept. When the file cannot be read at all, the last saved
/// settings stand in: an unreadable file must never pass for one whose
/// per-key values were all removed (2026-10-22 audit ISSUE-005).
pub(super) fn save_base(disk: Option<Settings>, saved: Settings) -> Settings {
    disk.unwrap_or(saved)
}

/// Mouse filter switch: applies at once (installs or removes the mouse
/// hook) and is saved right away, like the protection switch.
pub(crate) fn set_mouse_filter(on: bool) -> bool {
    crate::input::hook::set_mouse_filter(on);
    persist_with(|s| s.filter_mouse = on)
}

/// Mouse buttons' block window (Keys tab): applied and saved at once.
/// Returns the value used (clamped) and whether it reached disk.
pub(crate) fn set_mouse_threshold(ms: u64) -> (u64, bool) {
    let ms = ms.clamp(1, 1000);
    if let Some(shared) = SHARED.get() {
        shared.lock().unwrap().debouncer.set_mouse_threshold_ms(ms);
    }
    let written = persist_with(|s| s.mouse_threshold_ms = ms);
    history::invalidate();
    (ms, written)
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
