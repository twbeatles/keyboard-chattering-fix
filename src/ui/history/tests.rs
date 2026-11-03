//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

use crate::{debounce::LogEvent, i18n::strings};
use std::time::Duration;

use super::clock::{Clock, DAY_MS, civil_from_days, days_from_civil};
use super::csv::{csv_field, render_csv};
use super::rows::{KeyTotals, render_events, render_info, render_keys};
use super::table::columns;
use super::view::HistoryView;
use crate::i18n::Language;

fn ev(ms: u64, vk: u8, blocked: bool, gap: Option<u64>) -> LogEvent {
    LogEvent {
        time: Duration::from_millis(ms),
        vk,
        blocked,
        gap_ms: gap,
    }
}

fn sample() -> Vec<LogEvent> {
    vec![
        ev(1_000, 0x41, false, None),
        ev(1_020, 0x41, true, Some(12)),
        ev(2_000, 0x20, true, Some(30)),
        ev(3_000, 0x42, false, None),
    ]
}

/// 1970-01-01 00:00 local at startup, read 5 s later (same day).
fn epoch_clock() -> Clock {
    Clock::new(5_000, Duration::from_millis(5_000))
}

#[test]
fn day_numbers_round_trip_through_dates() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    assert_eq!(days_from_civil(1969, 12, 31), -1);
    for (y, m, d) in [(2024, 2, 29), (2026, 10, 26), (2100, 3, 1), (1999, 12, 31)] {
        assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
    }
}

#[test]
fn times_carry_the_date_once_the_day_changed() {
    // Started 2026-10-25 23:59:59.500 local; now 2026-10-26 00:00:10.
    let day = days_from_civil(2026, 10, 26);
    let now = day * DAY_MS + 10_000;
    let clock = Clock::new(now, Duration::from_millis(10_500));
    let before_midnight = Duration::from_millis(200);
    let after_midnight = Duration::from_millis(1_500);
    assert_eq!(clock.list_time(before_midnight), "10-25 23:59:59");
    assert_eq!(clock.list_time(after_midnight), "00:00:01.000");
    assert_eq!(clock.full_time(before_midnight), "2026-10-25 23:59:59.700");
    assert_eq!(clock.full_time(after_midnight), "2026-10-26 00:00:01.000");
}

#[test]
fn blocked_view_lists_only_blocked_newest_first() {
    let s = strings(Language::Ko);
    let rows = render_events(s, &sample(), HistoryView::Blocked, "", &epoch_clock());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cells, ["00:00:02.000", "Space", "막음", "30 ms"]);
    assert_eq!(rows[1].cells[1], "A");
    assert!(rows.iter().all(|r| !r.highlight));
}

#[test]
fn all_view_highlights_blocked_rows_and_shows_dash_gap() {
    let s = strings(Language::En);
    let rows = render_events(s, &sample(), HistoryView::All, "", &epoch_clock());
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].cells, ["00:00:03.000", "B", "Passed", "-"]);
    let highlighted: Vec<bool> = rows.iter().map(|r| r.highlight).collect();
    assert_eq!(highlighted, [false, true, true, false]);
}

#[test]
fn key_query_filters_events() {
    let s = strings(Language::En);
    let clock = epoch_clock();
    let rows = render_events(s, &sample(), HistoryView::All, "space", &clock);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells[1], "Space");
    assert!(render_events(s, &sample(), HistoryView::All, "F12", &clock).is_empty());
}

fn totals() -> KeyTotals {
    let mut t = KeyTotals {
        presses: [0; 256],
        blocked: [0; 256],
        thresholds: [(50, false); 256],
    };
    t.presses[0x41] = 100;
    t.blocked[0x41] = 5;
    t.presses[0x20] = 40;
    t.blocked[0x20] = 10;
    t.presses[0x42] = 300;
    t.thresholds[0x20] = (80, true);
    t
}

#[test]
fn key_summary_sorts_by_blocked_and_marks_overrides() {
    let s = strings(Language::Ko);
    let rows = render_keys(s, &totals(), "");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].cells,
        ["Space", "40", "10", "25.0%", "80 ms (개별)"]
    );
    assert_eq!(rows[1].cells, ["A", "100", "5", "5.0%", "50 ms"]);
    assert_eq!(rows[2].cells[0], "B");
    assert_eq!(
        rows.iter().map(|r| r.highlight).collect::<Vec<_>>(),
        [true, true, false]
    );
}

#[test]
fn key_summary_skips_unused_keys_and_honours_query() {
    let s = strings(Language::En);
    let empty = KeyTotals {
        presses: [0; 256],
        blocked: [0; 256],
        thresholds: [(50, false); 256],
    };
    assert!(render_keys(s, &empty, "").is_empty());
    let rows = render_keys(s, &totals(), "b");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells[0], "B");
}

#[test]
fn info_line_explains_missing_all_input_recording() {
    let s = strings(Language::Ko);
    let off = render_info(s, HistoryView::All, 3, false);
    assert!(off.starts_with("3개 표시 · "));
    assert!(off.contains("모든 키 입력 기록"));
    let on = render_info(s, HistoryView::All, 3, true);
    assert_ne!(on, off);
    assert!(render_info(s, HistoryView::Keys, 0, false).contains("유지"));
}

#[test]
fn csv_lists_events_oldest_first_then_the_key_summary() {
    let s = strings(Language::En);
    let csv = render_csv(s, &sample(), &totals(), &epoch_clock());
    assert!(csv.starts_with('\u{FEFF}'));
    let lines: Vec<&str> = csv.trim_start_matches('\u{FEFF}').split("\r\n").collect();
    assert_eq!(lines[0], "Time,Key,Result,Gap");
    assert_eq!(lines[1], "1970-01-01 00:00:01.000,A,Passed,-");
    assert_eq!(lines[4], "1970-01-01 00:00:03.000,B,Passed,-");
    assert_eq!(lines[5], "");
    assert_eq!(lines[6], "Key,Presses,Blocked,Rate,Block window");
    assert_eq!(lines[7], "Space,40,10,25.0%,80 ms (custom)");
    // Fields with separators or quotes are quoted.
    assert_eq!(csv_field("a,b"), "\"a,b\"");
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(csv_field(","), "\",\"");
}

#[test]
fn column_count_matches_rendered_cells() {
    let s = strings(Language::En);
    let events = render_events(s, &sample(), HistoryView::All, "", &epoch_clock());
    assert_eq!(columns(s, HistoryView::All).len(), events[0].cells.len());
    let keys = render_keys(s, &totals(), "");
    assert_eq!(columns(s, HistoryView::Keys).len(), keys[0].cells.len());
}
