//! Keys & mouse tab: the mouse-button switch and per-key block windows,
//! each with a suggestion computed from the gaps measured for that key.
//!
//! Rendering (`render_rows`, `advice_cell`, `advice_line`, `prefill_ms`) is
//! pure and unit-tested. The list is a virtual (`LVS_OWNERDATA`) list view
//! like the History tab's: the timer re-renders only while this tab is
//! visible and the counters moved, and locks are never held across Win32
//! calls.

use super::super::keys::selectable_vks;
use crate::{
    debounce::GAP_BUCKETS,
    i18n::strings,
    recommend::{Advice, recommend},
};

use super::model::{
    KeysSnapshot, advice_line, changes_text, prefill_ms, render_rows, suggested_changes,
};
use super::table::{columns, picker_order};
use crate::debounce::GAP_BUCKET_MS;
use crate::i18n::Language;

#[test]
fn picker_starts_on_letters_and_ends_on_mouse_buttons() {
    let order = picker_order(selectable_vks());
    assert_eq!(order.len(), selectable_vks().len());
    assert_eq!(order[0], 0x41, "A comes first");
    assert_eq!(order[26], 0x30, "digits follow Z");
    assert_eq!(order[order.len() - 5..], [0x01, 0x02, 0x04, 0x05, 0x06]);
}

fn snapshot() -> Box<KeysSnapshot> {
    let mut snap = KeysSnapshot::empty();
    snap.thresholds = [(50, false); 256];
    // A: chattery, clean separation -> suggestion 20 ms.
    snap.presses[0x41] = 120;
    snap.blocked[0x41] = 6;
    for (gap, n) in [(3u64, 6u32), (12, 3), (60, 20), (400, 80)] {
        snap.hist[0x41][((gap / GAP_BUCKET_MS) as usize).min(GAP_BUCKETS - 1)] += n;
    }
    // Space: override, nothing measured yet.
    snap.thresholds[0x20] = (80, true);
    // B: pressed a lot, never chattered -> not listed.
    snap.presses[0x42] = 300;
    snap.hist[0x42][GAP_BUCKETS - 1] = 300;
    // Left mouse: gaps overlap, nothing blocked -> listed to check.
    for slot in snap.hist[0x01].iter_mut().take(40) {
        *slot = 1;
    }
    snap
}

#[test]
fn rows_list_overrides_blocked_and_actionable_keys_only() {
    let s = strings(Language::En);
    let rows = render_rows(s, &snapshot());
    let vks: Vec<u8> = rows.iter().map(|r| r.vk).collect();
    // Blocked first, then the override, then by key code.
    assert_eq!(vks, [0x41, 0x20, 0x01]);
    assert_eq!(rows[0].cells, ["A", "50 ms", "20 ms", "6"]);
    assert_eq!(rows[1].cells, ["Space", "80 ms (custom)", "Need data", "0"]);
    assert_eq!(rows[2].cells[2], "Check switch");
    assert_eq!(
        rows.iter().map(|r| r.highlight).collect::<Vec<_>>(),
        [true, false, false]
    );
    assert_eq!(rows[0].cells.len(), columns(s).len());
}

/// A key whose chatter was measured in an earlier run (restored gaps)
/// gets its suggestion right away, not "Need data"; a key set to 0 is
/// shown as not filtered.
#[test]
fn restored_gaps_suggest_at_once_and_zero_reads_not_filtered() {
    let s = strings(Language::En);
    let mut snap = snapshot();
    snap.thresholds[0x20] = (0, true);
    let rows = render_rows(s, &snap);
    assert_eq!(rows[0].cells[2], "20 ms");
    assert_eq!(rows[1].cells, ["Space", "Not filtered", "Need data", "0"]);
    assert_eq!(
        strings(Language::Ko).threshold_off,
        render_rows(strings(Language::Ko), &snap)[1].cells[1]
    );
}

#[test]
fn advice_lines_name_the_key_and_fill_every_value() {
    let s = strings(Language::Ko);
    let snap = snapshot();
    let line = advice_line(s, 0x41, recommend(&snap.hist[0x41]));
    assert!(line.starts_with("A: 권장 20 ms"), "{line}");
    assert!(line.contains("15 ms 미만") && line.contains("60 ms 이상"));
    let need = advice_line(s, 0x20, recommend(&snap.hist[0x20]));
    assert!(need.contains("(0/20회)"), "{need}");
    for lang in Language::ALL.iter().copied() {
        let s = strings(lang);
        for advice in [
            Advice::NeedMoreData { have: 3, need: 20 },
            Advice::NoChatter,
            Advice::Suggest {
                ms: 20,
                chatter_below_ms: 15,
                normal_from_ms: None,
            },
            Advice::Suggest {
                ms: 20,
                chatter_below_ms: 15,
                normal_from_ms: Some(40),
            },
            Advice::Overlap,
        ] {
            let line = advice_line(s, 0x01, advice);
            assert!(line.starts_with("Mouse Left: "), "{line}");
            assert!(!line.contains("{}"), "{lang:?}: {line}");
        }
    }
}

/// "Apply suggestions" takes every key with a differing suggestion and
/// leaves keys set to 0 (not filtered) alone.
#[test]
fn apply_all_changes_suggested_keys_but_not_unfiltered_ones() {
    let mut snap = snapshot();
    // D: chatter at 0-4 ms, real presses from 35 ms -> 10 ms.
    for (gap, n) in [(1u64, 5u32), (35, 3), (80, 40)] {
        snap.hist[0x44][(gap / GAP_BUCKET_MS) as usize] += n;
    }
    assert_eq!(suggested_changes(&snap), [(0x41, 50, 20), (0x44, 50, 10)]);
    // A key already at its suggestion has nothing to change.
    snap.thresholds[0x41] = (20, true);
    snap.thresholds[0x44] = (0, true);
    assert_eq!(suggested_changes(&snap), []);

    snap.thresholds[0x41] = (50, false);
    for lang in Language::ALL.iter().copied() {
        let text = changes_text(strings(lang), &suggested_changes(&snap));
        assert!(text.contains("A: 50 → 20 ms"), "{lang:?}: {text}");
        assert!(!text.contains("{}"), "{lang:?}: {text}");
    }
    let many: Vec<(u8, u64, u64)> = (0x41..0x50).map(|vk| (vk, 35, 20)).collect();
    let text = changes_text(strings(Language::En), &many);
    assert!(text.starts_with("Apply the suggested window to 15 keys?"));
    assert_eq!(
        text.matches(
            " ms
"
        )
        .count(),
        12,
        "{text}"
    );
    assert!(text.contains("…"));
}

#[test]
fn prefill_takes_the_suggestion_else_the_current_window() {
    let snap = snapshot();
    assert_eq!(prefill_ms(&snap, 0x41), 20);
    assert_eq!(prefill_ms(&snap, 0x20), 80);
    assert_eq!(prefill_ms(&snap, 0x01), 50);
}
