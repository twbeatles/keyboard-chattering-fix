//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

use super::super::state::current_lang;
use super::clock::Clock;
use super::rows::{KeyTotals, event_rows, render_keys};
use super::view::HistoryView;
use crate::{
    app::state::SHARED,
    debounce::LogEvent,
    i18n::{Strings, strings},
};

pub(super) fn csv_field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

pub(super) fn csv_row(out: &mut String, cells: &[String]) {
    let line: Vec<String> = cells.iter().map(|c| csv_field(c)).collect();
    out.push_str(&line.join(","));
    out.push_str("\r\n");
}

/// The event log (oldest first) and the per-key summary as CSV, with a
/// UTF-8 BOM so spreadsheet apps pick the right encoding.
pub(crate) fn render_csv(
    s: &Strings,
    events: &[LogEvent],
    totals: &KeyTotals,
    clock: &Clock,
) -> String {
    let mut out = String::from("\u{FEFF}");
    let header = |cols: &[&str]| cols.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    csv_row(
        &mut out,
        &header(&[s.col_time, s.col_key, s.col_result, s.col_gap]),
    );
    for row in event_rows(s, events, HistoryView::All, "", |t| clock.full_time(t))
        .into_iter()
        .rev()
    {
        csv_row(&mut out, &row.cells);
    }
    out.push_str("\r\n");
    csv_row(
        &mut out,
        &header(&[
            s.col_key,
            s.col_presses,
            s.col_blocked,
            s.col_rate,
            s.col_threshold,
        ]),
    );
    for row in render_keys(s, totals, "") {
        csv_row(&mut out, &row.cells);
    }
    out
}

/// CSV of what is recorded right now (File > Export history).
pub(crate) fn csv_snapshot() -> Option<String> {
    let shared = SHARED.get()?;
    let (events, totals) = {
        let g = shared.lock().unwrap();
        let d = &g.debouncer;
        let mut thresholds = [(0u64, false); 256];
        for (vk, slot) in thresholds.iter_mut().enumerate() {
            *slot = d.key_threshold_ms(vk as u8);
        }
        let events: Vec<LogEvent> = d.log_events.iter().copied().collect();
        (
            events,
            Box::new(KeyTotals {
                presses: d.per_key_presses,
                blocked: d.per_key_blocked,
                thresholds,
            }),
        )
    };
    Some(render_csv(
        strings(current_lang()),
        &events,
        &totals,
        &Clock::now(),
    ))
}
