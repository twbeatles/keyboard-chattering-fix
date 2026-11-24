//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

use std::time::Duration;

use super::super::keys::{key_matches, vk_name};
use super::clock::Clock;
use super::view::{HistoryView, Row};
use crate::{
    debounce::LogEvent,
    i18n::{Strings, fill},
};

/// Event rows, newest first, filtered by view and key query.
pub(crate) fn render_events(
    s: &Strings,
    events: &[LogEvent],
    view: HistoryView,
    query: &str,
    clock: &Clock,
) -> Vec<Row> {
    event_rows(s, events, view, query, |t| clock.list_time(t))
}

pub(super) fn event_rows(
    s: &Strings,
    events: &[LogEvent],
    view: HistoryView,
    query: &str,
    time: impl Fn(Duration) -> String,
) -> Vec<Row> {
    events
        .iter()
        .rev()
        .filter(|e| view != HistoryView::Blocked || e.blocked)
        .filter(|e| key_matches(e.vk, query))
        .map(|e| Row {
            cells: vec![
                time(e.time),
                vk_name(e.vk),
                (if e.blocked {
                    s.result_blocked
                } else {
                    s.result_passed
                })
                .to_string(),
                e.gap_ms
                    .map_or_else(|| "-".to_string(), |g| format!("{g} ms")),
            ],
            // In "blocked only" every row is blocked; color adds nothing.
            highlight: e.blocked && view == HistoryView::All,
            vk: Some(e.vk),
        })
        .collect()
}

/// Blocked presses in the trailing 7-day window, per virtual key, from the
/// persisted day history. Reads `SAVED` first and never nests it under the
/// hook lock: the lock order is SAVED before SHARED everywhere.
pub(super) fn week_blocked_now() -> [u64; 256] {
    let Some(saved) = super::super::state::SAVED.get() else {
        return [0; 256];
    };
    let saved = saved.lock().unwrap();
    let today = crate::config::today_yyyymmdd();
    let end = crate::recommend::ymd_to_days(today).unwrap_or(0);
    let sums =
        crate::recommend::window_sums(&saved.day_history, end, crate::recommend::TREND_WINDOW_DAYS);
    let mut week = [0u64; 256];
    for (i, s) in sums.iter().enumerate() {
        week[i] = s.1;
    }
    week
}

/// A key's block window as shown in the lists: `50 ms`, `80 ms (custom)`,
/// or "Not filtered" for a per-key 0 and for Shift/Ctrl by default.
pub(crate) fn threshold_cell(s: &Strings, ms: u64, custom: bool) -> String {
    match (ms, custom) {
        (0, _) => s.threshold_off.to_string(),
        (ms, true) => fill(s.threshold_custom_fmt, &[&ms.to_string()]),
        (ms, false) => format!("{ms} ms"),
    }
}

/// Cumulative per-key counters plus the effective threshold of each key.
pub(crate) struct KeyTotals {
    pub presses: [u64; 256],
    pub blocked: [u64; 256],
    /// Blocked presses in the trailing 7-day window, per key.
    pub week_blocked: [u64; 256],
    /// (ms, is a per-key override)
    pub thresholds: [(u64, bool); 256],
}

/// Per-key summary rows: keys that were ever pressed, most-blocked first.
pub(crate) fn render_keys(s: &Strings, t: &KeyTotals, query: &str) -> Vec<Row> {
    let mut vks: Vec<u8> = (0..=255u8)
        .filter(|&vk| t.presses[vk as usize] > 0 || t.blocked[vk as usize] > 0)
        .filter(|&vk| key_matches(vk, query))
        .collect();
    vks.sort_by(|&a, &b| {
        let (a, b) = (a as usize, b as usize);
        t.blocked[b]
            .cmp(&t.blocked[a])
            .then(t.presses[b].cmp(&t.presses[a]))
            .then(a.cmp(&b))
    });
    vks.into_iter()
        .map(|vk| {
            let i = vk as usize;
            let rate = if t.presses[i] == 0 {
                0.0
            } else {
                t.blocked[i] as f64 * 100.0 / t.presses[i] as f64
            };
            let (ms, custom) = t.thresholds[i];
            Row {
                cells: vec![
                    vk_name(vk),
                    t.presses[i].to_string(),
                    t.blocked[i].to_string(),
                    format!("{rate:.1}%"),
                    threshold_cell(s, ms, custom),
                    t.week_blocked[i].to_string(),
                ],
                highlight: t.blocked[i] > 0,
                vk: Some(vk),
            }
        })
        .collect()
}

/// The line under the list: row count plus a hint matching the view.
pub(crate) fn render_info(
    s: &Strings,
    view: HistoryView,
    shown: usize,
    record_all: bool,
) -> String {
    let note = match view {
        HistoryView::Blocked => s.history_note_blocked,
        HistoryView::All if record_all => s.history_note_all_on,
        HistoryView::All => s.history_note_all_off,
        HistoryView::Keys => s.history_note_keys,
    };
    format!(
        "{} · {}",
        fill(s.history_count_fmt, &[&shown.to_string()]),
        note
    )
}
