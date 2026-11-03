//! Keys & mouse tab: the mouse-button switch and per-key block windows,
//! each with a suggestion computed from the gaps measured for that key.
//!
//! Rendering (`render_rows`, `advice_cell`, `advice_line`, `prefill_ms`) is
//! pure and unit-tested. The list is a virtual (`LVS_OWNERDATA`) list view
//! like the History tab's: the timer re-renders only while this tab is
//! visible and the counters moved, and locks are never held across Win32
//! calls.

use std::sync::{Mutex, OnceLock, atomic::AtomicBool};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::Gdi::InvalidateRect,
    UI::{Controls::*, WindowsAndMessaging::*},
};

use super::super::{
    history::{set_columns, setup_list, theme_list},
    ids::*,
    keys::{selectable_vks, vk_name},
    layout::{Page, current_page, showing_cleaning},
    state::current_lang,
    text::{fill_combo, set_text},
};
use super::actions::{select_row, update_advice};
use super::model::{KeyRow, KeysSnapshot, render_rows};
use crate::{
    app::state::SHARED,
    i18n::{Strings, strings},
};

pub(super) fn columns(s: &Strings) -> [(&'static str, i32, bool); 4] {
    [
        (s.col_key, 104, false),
        (s.col_threshold, 118, true),
        (s.col_reco, 96, true),
        (s.col_blocked, 64, true),
    ]
}

// --- Window glue (UI thread) ---------------------------------------------

/// Change key of the last render: (total presses, blocked, global window).
pub(super) type RenderKey = (u64, u64, u64);

pub(super) struct State {
    pub(super) rows: Vec<KeyRow>,
    pub(super) snap: Option<Box<KeysSnapshot>>,
    pub(super) selected: Option<u8>,
    pub(super) last_key: Option<RenderKey>,
    /// Text last written to the advice line (skip identical rewrites).
    pub(super) advice_text: String,
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    rows: Vec::new(),
    snap: None,
    selected: None,
    last_key: None,
    advice_text: String::new(),
});
/// Set while the program moves the list selection itself, so the
/// resulting `LVN_ITEMCHANGED` is not taken for a user pick.
pub(super) static SUPPRESS_SELECT: AtomicBool = AtomicBool::new(false);

/// The picker's keys, in combo order.
pub(super) fn picker_vks() -> &'static [u8] {
    static VKS: OnceLock<Vec<u8>> = OnceLock::new();
    VKS.get_or_init(|| picker_order(selectable_vks()))
}

/// Letters and digits first (the keys that usually chatter), then the
/// other keys in code order, mouse buttons last: the picker starts on "A"
/// instead of a mouse button in this keyboard-first tool.
pub(super) fn picker_order(mut vks: Vec<u8>) -> Vec<u8> {
    vks.sort_by_key(|&vk| match vk {
        0x41..=0x5A => 0,
        0x30..=0x39 => 1,
        0x01 | 0x02 | 0x04..=0x06 => 3,
        _ => 2,
    });
    vks
}

pub(super) unsafe fn list(hwnd: HWND) -> Option<HWND> {
    unsafe { GetDlgItem(Some(hwnd), ID_KEYS_LIST).ok() }
}

/// Copy everything the tab shows. The buffers are allocated before the
/// lock is taken; under it only plain copies run.
pub(super) fn take_snapshot() -> Option<(RenderKey, Box<KeysSnapshot>)> {
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
    unsafe {
        if let Some(lv) = list(hwnd) {
            setup_list(lv);
        }
        let names: Vec<String> = picker_vks().iter().map(|&vk| vk_name(vk)).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        fill_combo(hwnd, ID_KEYS_COMBO, &refs, 0);
        for id in [ID_KEYS_EDIT, ID_MOUSE_MS_EDIT] {
            if let Ok(edit) = GetDlgItem(Some(hwnd), id) {
                SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(4)), Some(LPARAM(0)));
            }
        }
        let mouse_ms = SHARED
            .get()
            .map(|g| g.lock().unwrap().debouncer.mouse_threshold_ms());
        if let Some(ms) = mouse_ms {
            set_text(hwnd, ID_MOUSE_MS_EDIT, &ms.to_string());
        }
    }
}

pub(crate) unsafe fn apply_theme(hwnd: HWND) {
    unsafe {
        if let Some(lv) = list(hwnd) {
            theme_list(lv);
        }
    }
}

/// Retranslate the columns and every rendered text.
pub(crate) unsafe fn apply_language(hwnd: HWND) {
    unsafe {
        if let Some(lv) = list(hwnd) {
            set_columns(lv, &columns(strings(current_lang())));
        }
        STATE.lock().unwrap().advice_text.clear();
        refresh(hwnd, true);
        update_advice(hwnd);
    }
}

/// Force the next refresh to re-render (a window changed).
pub(crate) fn invalidate() {
    STATE.lock().unwrap().last_key = None;
}

/// Re-render when this tab is visible and something changed.
pub(crate) unsafe fn refresh(hwnd: HWND, force: bool) {
    unsafe {
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
}
