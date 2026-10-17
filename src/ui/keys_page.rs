//! Keys & mouse tab: the mouse-button switch and per-key block windows,
//! each with a suggestion computed from the gaps measured for that key.
//!
//! Rendering (`render_rows`, `advice_cell`, `advice_line`, `prefill_ms`) is
//! pure and unit-tested. The list is a virtual (`LVS_OWNERDATA`) list view
//! like the History tab's: the timer re-renders only while this tab is
//! visible and the counters moved, and locks are never held across Win32
//! calls.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::InvalidateRect,
    UI::{Controls::*, WindowsAndMessaging::*},
};

use super::{
    dialog::msg_box,
    history::{set_columns, setup_list, theme_list, write_cell},
    ids::*,
    keys::{selectable_vks, vk_name},
    layout::{current_page, showing_cleaning, Page},
    paint::{theme_is_dark, ThemeColors},
    settings_form,
    state::current_lang,
    text::{combo_sel, fill_combo, get_text, set_text},
};
use crate::{
    app::state::SHARED,
    debounce::{GapHistogram, GAP_BUCKETS},
    i18n::{fill, strings, Strings},
    recommend::{recommend, Advice},
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
    fn empty() -> Box<Self> {
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
                    if custom {
                        fill(s.threshold_custom_fmt, &[&ms.to_string()])
                    } else {
                        format!("{ms} ms")
                    },
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

fn columns(s: &Strings) -> [(&'static str, i32, bool); 4] {
    [
        (s.col_key, 104, false),
        (s.col_threshold, 118, true),
        (s.col_reco, 96, true),
        (s.col_blocked, 64, true),
    ]
}

// --- Window glue (UI thread) ---------------------------------------------

/// Change key of the last render: (total presses, blocked, global window).
type RenderKey = (u64, u64, u64);

struct State {
    rows: Vec<KeyRow>,
    snap: Option<Box<KeysSnapshot>>,
    selected: Option<u8>,
    last_key: Option<RenderKey>,
    /// Text last written to the advice line (skip identical rewrites).
    advice_text: String,
}

static STATE: Mutex<State> = Mutex::new(State {
    rows: Vec::new(),
    snap: None,
    selected: None,
    last_key: None,
    advice_text: String::new(),
});
/// Set while the program moves the list selection itself, so the
/// resulting `LVN_ITEMCHANGED` is not taken for a user pick.
static SUPPRESS_SELECT: AtomicBool = AtomicBool::new(false);

/// The picker's keys, in combo order.
fn picker_vks() -> &'static [u8] {
    static VKS: OnceLock<Vec<u8>> = OnceLock::new();
    VKS.get_or_init(selectable_vks)
}

unsafe fn list(hwnd: HWND) -> Option<HWND> {
    GetDlgItem(Some(hwnd), ID_KEYS_LIST).ok()
}

/// Copy everything the tab shows. The buffers are allocated before the
/// lock is taken; under it only plain copies run.
fn take_snapshot() -> Option<(RenderKey, Box<KeysSnapshot>)> {
    let shared = SHARED.get()?;
    let mut snap = KeysSnapshot::empty();
    let g = shared.lock().unwrap();
    let d = &g.debouncer;
    snap.presses = d.per_key_presses;
    snap.blocked = d.per_key_blocked;
    for vk in 0..=255u8 {
        snap.thresholds[vk as usize] = d.key_threshold_ms(vk);
        snap.hist[vk as usize] = *d.gap_histogram(vk);
    }
    let key = (d.total_presses, d.blocked_presses, d.threshold_ms());
    Some((key, snap))
}

/// One-time setup after the controls exist.
pub(crate) unsafe fn init(hwnd: HWND) {
    if let Some(lv) = list(hwnd) {
        setup_list(lv);
    }
    let names: Vec<String> = picker_vks().iter().map(|&vk| vk_name(vk)).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    fill_combo(hwnd, ID_KEYS_COMBO, &refs, 0);
    if let Ok(edit) = GetDlgItem(Some(hwnd), ID_KEYS_EDIT) {
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(4)), Some(LPARAM(0)));
    }
}

pub(crate) unsafe fn apply_theme(hwnd: HWND) {
    if let Some(lv) = list(hwnd) {
        theme_list(lv);
    }
}

/// Retranslate the columns and every rendered text.
pub(crate) unsafe fn apply_language(hwnd: HWND) {
    if let Some(lv) = list(hwnd) {
        set_columns(lv, &columns(strings(current_lang())));
    }
    STATE.lock().unwrap().advice_text.clear();
    refresh(hwnd, true);
    update_advice(hwnd);
}

/// Force the next refresh to re-render (a window changed).
pub(crate) fn invalidate() {
    STATE.lock().unwrap().last_key = None;
}

/// Re-render when this tab is visible and something changed.
pub(crate) unsafe fn refresh(hwnd: HWND, force: bool) {
    if current_page() != Page::Keys || showing_cleaning() {
        return;
    }
    let last_key = STATE.lock().unwrap().last_key;
    let Some(shared) = SHARED.get() else {
        return;
    };
    let key = {
        let g = shared.lock().unwrap();
        let d = &g.debouncer;
        (d.total_presses, d.blocked_presses, d.threshold_ms())
    };
    if !force && last_key == Some(key) {
        return;
    }
    let Some((key, snap)) = take_snapshot() else {
        return;
    };
    let rows = render_rows(strings(current_lang()), &snap);
    let count = rows.len();
    let selected_row = {
        let mut st = STATE.lock().unwrap();
        st.rows = rows;
        st.snap = Some(snap);
        st.last_key = Some(key);
        st.selected
            .and_then(|vk| st.rows.iter().position(|r| r.vk == vk))
    };
    if let Some(lv) = list(hwnd) {
        SendMessageW(
            lv,
            LVM_SETITEMCOUNT,
            Some(WPARAM(count)),
            Some(LPARAM(LVSICF_NOSCROLL as isize)),
        );
        let _ = InvalidateRect(Some(lv), None, false);
    }
    // Rows may have moved: keep the highlight on the selected key.
    select_row(hwnd, selected_row);
    update_advice(hwnd);
}

/// Select (and scroll to) one list row, or clear the selection.
unsafe fn select_row(hwnd: HWND, row: Option<usize>) {
    let Some(lv) = list(hwnd) else {
        return;
    };
    SUPPRESS_SELECT.store(true, Ordering::Relaxed);
    let mask = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let clear = LVITEMW {
        stateMask: mask,
        ..Default::default()
    };
    SendMessageW(
        lv,
        LVM_SETITEMSTATE,
        Some(WPARAM(usize::MAX)),
        Some(LPARAM(&clear as *const _ as isize)),
    );
    if let Some(row) = row {
        let set = LVITEMW {
            stateMask: mask,
            state: mask,
            ..Default::default()
        };
        SendMessageW(
            lv,
            LVM_SETITEMSTATE,
            Some(WPARAM(row)),
            Some(LPARAM(&set as *const _ as isize)),
        );
        SendMessageW(lv, LVM_ENSUREVISIBLE, Some(WPARAM(row)), Some(LPARAM(0)));
    }
    SUPPRESS_SELECT.store(false, Ordering::Relaxed);
}

/// Rewrite the advice line for the selected key when its text changed.
unsafe fn update_advice(hwnd: HWND) {
    let s = strings(current_lang());
    let text = {
        let mut st = STATE.lock().unwrap();
        let text = match (st.selected, st.snap.as_deref()) {
            (Some(vk), Some(snap)) => advice_line(s, vk, recommend(&snap.hist[vk as usize])),
            _ => s.advice_pick.to_string(),
        };
        if text == st.advice_text {
            return;
        }
        st.advice_text = text.clone();
        text
    };
    set_text(hwnd, ID_KEYS_ADVICE, &text);
}

/// Make `vk` the key being edited: picker, edit box, advice, and (when
/// `sync_list`) the list selection follow.
pub(crate) unsafe fn select_key(hwnd: HWND, vk: u8, sync_list: bool) {
    if STATE.lock().unwrap().snap.is_none() {
        if let Some((key, snap)) = take_snapshot() {
            let mut st = STATE.lock().unwrap();
            st.snap = Some(snap);
            st.last_key = Some(key);
        }
    }
    let (prefill, row) = {
        let mut st = STATE.lock().unwrap();
        st.selected = Some(vk);
        (
            st.snap.as_deref().map(|snap| prefill_ms(snap, vk)),
            st.rows.iter().position(|r| r.vk == vk),
        )
    };
    if let Some(index) = picker_vks().iter().position(|&k| k == vk) {
        if let Ok(combo) = GetDlgItem(Some(hwnd), ID_KEYS_COMBO) {
            SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(index)), Some(LPARAM(0)));
        }
    }
    if let Some(ms) = prefill {
        set_text(hwnd, ID_KEYS_EDIT, &ms.to_string());
    }
    if sync_list {
        select_row(hwnd, row);
    }
    update_advice(hwnd);
}

pub(crate) unsafe fn on_combo_changed(hwnd: HWND) {
    if let Some(&vk) = picker_vks().get(combo_sel(hwnd, ID_KEYS_COMBO)) {
        select_key(hwnd, vk, true);
    }
}

/// The key in the picker (always valid: the combo starts on its first item).
unsafe fn picked_vk(hwnd: HWND) -> Option<u8> {
    picker_vks().get(combo_sel(hwnd, ID_KEYS_COMBO)).copied()
}

unsafe fn after_change(hwnd: HWND, vk: u8, written: bool) {
    invalidate();
    refresh(hwnd, true);
    select_key(hwnd, vk, true);
    if !written {
        let s = strings(current_lang());
        msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
    }
}

/// "Apply": give the picked key the window in the edit box.
pub(crate) unsafe fn apply_override(hwnd: HWND) {
    let Some(vk) = picked_vk(hwnd) else {
        return;
    };
    let Ok(ms) = get_text(hwnd, ID_KEYS_EDIT).parse::<u64>() else {
        let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONWARNING);
        return;
    };
    let written = settings_form::set_key_override(vk, Some(ms));
    after_change(hwnd, vk, written);
}

/// "Default": the picked key follows the global window again.
pub(crate) unsafe fn clear_override(hwnd: HWND) {
    let Some(vk) = picked_vk(hwnd) else {
        return;
    };
    let written = settings_form::set_key_override(vk, None);
    after_change(hwnd, vk, written);
}

pub(crate) unsafe fn on_mouse_toggled(hwnd: HWND, on: bool) {
    if !settings_form::set_mouse_filter(on) {
        let s = strings(current_lang());
        msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
    }
}

/// `WM_NOTIFY` from the list: cell text, row color, and user selection.
pub(crate) unsafe fn on_notify(hwnd: HWND, lparam: LPARAM) -> Option<LRESULT> {
    let hdr = &*(lparam.0 as *const NMHDR);
    if hdr.idFrom != ID_KEYS_LIST as usize {
        return None;
    }
    match hdr.code {
        LVN_GETDISPINFOW => {
            let info = &mut *(lparam.0 as *mut NMLVDISPINFOW);
            if (info.item.mask & LVIF_TEXT).0 != 0
                && !info.item.pszText.is_null()
                && info.item.cchTextMax > 0
            {
                let st = STATE.lock().unwrap();
                let text = st
                    .rows
                    .get(info.item.iItem as usize)
                    .and_then(|r| r.cells.get(info.item.iSubItem as usize))
                    .map(String::as_str)
                    .unwrap_or("");
                write_cell(info, text);
            }
            Some(LRESULT(0))
        }
        NM_CUSTOMDRAW => {
            let cd = &mut *(lparam.0 as *mut NMLVCUSTOMDRAW);
            if cd.nmcd.dwDrawStage == CDDS_PREPAINT {
                return Some(LRESULT(CDRF_NOTIFYITEMDRAW as isize));
            }
            if cd.nmcd.dwDrawStage == CDDS_ITEMPREPAINT {
                let highlight = STATE
                    .lock()
                    .unwrap()
                    .rows
                    .get(cd.nmcd.dwItemSpec)
                    .is_some_and(|r| r.highlight);
                if highlight {
                    cd.clrText = ThemeColors::new(theme_is_dark()).warn;
                    return Some(LRESULT(CDRF_NEWFONT as isize));
                }
            }
            Some(LRESULT(CDRF_DODEFAULT as isize))
        }
        LVN_ITEMCHANGED => {
            let nm = &*(lparam.0 as *const NMLISTVIEW);
            let picked = !SUPPRESS_SELECT.load(Ordering::Relaxed)
                && nm.iItem >= 0
                && nm.uNewState & LVIS_SELECTED.0 != 0
                && nm.uOldState & LVIS_SELECTED.0 == 0;
            if picked {
                let vk = STATE
                    .lock()
                    .unwrap()
                    .rows
                    .get(nm.iItem as usize)
                    .map(|r| r.vk);
                if let Some(vk) = vk {
                    select_key(hwnd, vk, false);
                }
            }
            Some(LRESULT(0))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debounce::GAP_BUCKET_MS;
    use crate::i18n::Language;

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
        for i in 0..40 {
            snap.hist[0x01][i] = 1;
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

    #[test]
    fn prefill_takes_the_suggestion_else_the_current_window() {
        let snap = snapshot();
        assert_eq!(prefill_ms(&snap, 0x41), 20);
        assert_eq!(prefill_ms(&snap, 0x20), 80);
        assert_eq!(prefill_ms(&snap, 0x01), 50);
    }
}
