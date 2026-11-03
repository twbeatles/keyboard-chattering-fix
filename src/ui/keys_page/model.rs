//! Keys & mouse tab: the mouse-button switch and per-key block windows,
//! each with a suggestion computed from the gaps measured for that key.
//!
//! Rendering (`render_rows`, `advice_cell`, `advice_line`, `prefill_ms`) is
//! pure and unit-tested. The list is a virtual (`LVS_OWNERDATA`) list view
//! like the History tab's: the timer re-renders only while this tab is
//! visible and the counters moved, and locks are never held across Win32
//! calls.

use super::super::history::threshold_cell;
use super::super::keys::vk_name;
use crate::{
    debounce::{GAP_BUCKETS, GapHistogram},
    i18n::{Strings, fill},
    recommend::{Advice, recommend},
};

/// Per-key data the tab shows, copied under one short lock.
pub(crate) struct KeysSnapshot {
    pub presses: [u64; 256],
    pub blocked: [u64; 256],
    /// (ms, is a per-key override)
    pub thresholds: [(u64, bool); 256],
    pub hist: Box<[GapHistogram; 256]>,
}

impl KeysSnapshot {
    pub(super) fn empty() -> Box<Self> {
        Box::new(Self {
            presses: [0; 256],
            blocked: [0; 256],
            thresholds: [(0, false); 256],
            hist: vec![[0; GAP_BUCKETS]; 256]
                .into_boxed_slice()
                .try_into()
                .expect("256 histograms"),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeyRow {
    pub vk: u8,
    pub cells: Vec<String>,
    pub highlight: bool,
}

/// Short form of the advice for the list's "Suggested" column.
pub(crate) fn advice_cell(s: &Strings, advice: Advice) -> String {
    match advice {
        Advice::NeedMoreData { .. } => s.reco_cell_need.to_string(),
        Advice::NoChatter => s.reco_cell_ok.to_string(),
        Advice::Suggest { ms, .. } => format!("{ms} ms"),
        Advice::Overlap => s.reco_cell_overlap.to_string(),
    }
}

/// The sentence under the list explaining the advice for one key.
pub(crate) fn advice_line(s: &Strings, vk: u8, advice: Advice) -> String {
    let name = vk_name(vk);
    match advice {
        Advice::NeedMoreData { have, need } => fill(
            s.advice_need_fmt,
            &[&name, &have.to_string(), &need.to_string()],
        ),
        Advice::NoChatter => fill(s.advice_ok_fmt, &[&name]),
        Advice::Suggest {
            ms,
            chatter_below_ms,
            normal_from_ms: Some(normal),
        } => fill(
            s.advice_suggest_fmt,
            &[
                &name,
                &ms.to_string(),
                &chatter_below_ms.to_string(),
                &normal.to_string(),
            ],
        ),
        Advice::Suggest {
            ms,
            chatter_below_ms,
            normal_from_ms: None,
        } => fill(
            s.advice_suggest_only_fmt,
            &[&name, &ms.to_string(), &chatter_below_ms.to_string()],
        ),
        Advice::Overlap => fill(s.advice_overlap_fmt, &[&name]),
    }
}

/// Keys worth listing: a per-key override, a blocked press, or measured
/// gaps that call for action. Most-blocked first, then overrides, then by
/// key code. Every other key is reachable through the picker.
pub(crate) fn render_rows(s: &Strings, snap: &KeysSnapshot) -> Vec<KeyRow> {
    let mut rows: Vec<(u8, Advice)> = (0..=255u8)
        .map(|vk| (vk, recommend(&snap.hist[vk as usize])))
        .filter(|&(vk, advice)| {
            let i = vk as usize;
            snap.thresholds[i].1
                || snap.blocked[i] > 0
                || matches!(advice, Advice::Suggest { .. } | Advice::Overlap)
        })
        .collect();
    rows.sort_by(|&(a, _), &(b, _)| {
        let (a, b) = (a as usize, b as usize);
        snap.blocked[b]
            .cmp(&snap.blocked[a])
            .then(snap.thresholds[b].1.cmp(&snap.thresholds[a].1))
            .then(a.cmp(&b))
    });
    rows.into_iter()
        .map(|(vk, advice)| {
            let i = vk as usize;
            let (ms, custom) = snap.thresholds[i];
            KeyRow {
                vk,
                cells: vec![
                    vk_name(vk),
                    threshold_cell(s, ms, custom),
                    advice_cell(s, advice),
                    snap.blocked[i].to_string(),
                ],
                highlight: snap.blocked[i] > 0,
            }
        })
        .collect()
}

/// Value put in the edit box when a key is picked: the suggestion when
/// there is one (so Apply takes it), otherwise the key's current window.
pub(crate) fn prefill_ms(snap: &KeysSnapshot, vk: u8) -> u64 {
    match recommend(&snap.hist[vk as usize]) {
        Advice::Suggest { ms, .. } => ms,
        _ => snap.thresholds[vk as usize].0,
    }
}

/// What "Apply suggestions" changes: (key, current window, suggested
/// window) for every key whose suggestion differs from its window. Keys
/// set to 0 were left unfiltered on purpose and are skipped.
pub(crate) fn suggested_changes(snap: &KeysSnapshot) -> Vec<(u8, u64, u64)> {
    (0..=255u8)
        .filter_map(|vk| {
            let (cur, custom) = snap.thresholds[vk as usize];
            match recommend(&snap.hist[vk as usize]) {
                Advice::Suggest { ms, .. } if ms != cur && !(custom && cur == 0) => {
                    Some((vk, cur, ms))
                }
                _ => None,
            }
        })
        .collect()
}

/// Confirmation text listing the changes (`A: 35 -> 20 ms`), at most
/// `MAX_LISTED` lines.
pub(crate) fn changes_text(s: &Strings, changes: &[(u8, u64, u64)]) -> String {
    const MAX_LISTED: usize = 12;
    let mut lines: Vec<String> = changes
        .iter()
        .take(MAX_LISTED)
        .map(|&(vk, cur, ms)| format!("{}: {cur} → {ms} ms", vk_name(vk)))
        .collect();
    if changes.len() > MAX_LISTED {
        lines.push("…".to_string());
    }
    fill(
        s.apply_all_confirm_fmt,
        &[
            &changes.len().to_string(),
            &lines.join(
                "
",
            ),
        ],
    )
}
