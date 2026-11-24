//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use crate::{config::Settings, debounce::MeasureFrom};

use super::fields::clamp_u64;
use super::persist::{
    AUTOSAVE_INTERVAL_TICKS, LiveFingerprint, autosave_due, baseline_changed, save_base,
};

#[test]
fn save_starts_from_the_file_but_never_from_an_unreadable_one() {
    let mut saved = Settings::default();
    saved.per_key_ms[0x41] = Some(90);
    let mut disk = Settings::default();
    disk.per_key_ms[0x42] = Some(120);
    // Readable file: its per-key values win (edits made in the file).
    let base = save_base(Some(disk), saved.clone());
    assert_eq!(
        (base.per_key_ms[0x41], base.per_key_ms[0x42]),
        (None, Some(120))
    );
    // Unreadable file: the last saved values stay, nothing is wiped.
    let base = save_base(None, saved);
    assert_eq!(
        (base.per_key_ms[0x41], base.per_key_ms[0x42]),
        (Some(90), None)
    );
}

/// `on_save` never assigns the check timestamp: whatever `save_base`
/// yields is what gets saved, so a Save must not clobber the worker's
/// newer stamp (or an older one on disk).
#[test]
fn save_base_preserves_the_update_check_timestamp() {
    let saved = Settings {
        last_update_check_unix: 1000,
        ..Settings::default()
    };
    let disk = Settings {
        last_update_check_unix: 2000,
        ..Settings::default()
    };
    assert_eq!(
        save_base(Some(disk), saved.clone()).last_update_check_unix,
        2000
    );
    assert_eq!(save_base(None, saved).last_update_check_unix, 1000);
}

#[test]
fn only_a_switched_baseline_voids_the_measured_gaps() {
    assert!(baseline_changed(MeasureFrom::Release, MeasureFrom::Press));
    assert!(baseline_changed(MeasureFrom::Press, MeasureFrom::Release));
    assert!(!baseline_changed(
        MeasureFrom::Release,
        MeasureFrom::Release
    ));
}

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
