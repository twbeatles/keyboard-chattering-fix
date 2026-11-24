//! Keys & mouse tab: the mouse-button switch and per-key block windows,
//! each with a suggestion computed from the gaps measured for that key.
//!
//! Rendering (`render_rows`, `advice_cell`, `advice_line`, `prefill_ms`) is
//! pure and unit-tested. The list is a virtual (`LVS_OWNERDATA`) list view
//! like the History tab's: the timer re-renders only while this tab is
//! visible and the counters moved, and locks are never held across Win32
//! calls.

use std::sync::atomic::Ordering;

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{Controls::*, WindowsAndMessaging::*},
};

use super::super::{
    dialog::msg_box,
    history::write_cell,
    ids::*,
    paint::{ThemeColors, theme_is_dark},
    settings_form,
    state::{SAVED, current_lang},
    text::{combo_sel, get_text, set_text},
};
use super::model::{advice_line, changes_text, prefill_ms, suggested_changes};
use super::table::{STATE, SUPPRESS_SELECT, invalidate, list, picker_vks, refresh, take_snapshot};
use crate::{app::state::SHARED, i18n::strings, recommend::recommend};

/// Select (and scroll to) one list row, or clear the selection.
pub(super) unsafe fn select_row(hwnd: HWND, row: Option<usize>) {
    unsafe {
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
}

/// Rewrite the advice line for the selected key when its text changed.
pub(super) unsafe fn update_advice(hwnd: HWND) {
    unsafe {
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
}

/// Make `vk` the key being edited: picker, edit box, advice, and (when
/// `sync_list`) the list selection follow.
pub(crate) unsafe fn select_key(hwnd: HWND, vk: u8, sync_list: bool) {
    unsafe {
        if STATE.lock().unwrap().snap.is_none()
            && let Some((key, snap)) = take_snapshot()
        {
            let mut st = STATE.lock().unwrap();
            st.snap = Some(snap);
            st.last_key = Some(key);
        }
        let (prefill, row) = {
            let mut st = STATE.lock().unwrap();
            st.selected = Some(vk);
            (
                st.snap.as_deref().map(|snap| prefill_ms(snap, vk)),
                st.rows.iter().position(|r| r.vk == vk),
            )
        };
        if let Some(index) = picker_vks().iter().position(|&k| k == vk)
            && let Ok(combo) = GetDlgItem(Some(hwnd), ID_KEYS_COMBO)
        {
            SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(index)), Some(LPARAM(0)));
        }
        if let Some(ms) = prefill {
            set_text(hwnd, ID_KEYS_EDIT, &ms.to_string());
        }
        if sync_list {
            select_row(hwnd, row);
        }
        update_advice(hwnd);
    }
}

pub(crate) unsafe fn on_combo_changed(hwnd: HWND) {
    unsafe {
        if let Some(&vk) = picker_vks().get(combo_sel(hwnd, ID_KEYS_COMBO)) {
            select_key(hwnd, vk, true);
        }
    }
}

/// The key in the picker (always valid: the combo starts on its first item).
pub(super) unsafe fn picked_vk(hwnd: HWND) -> Option<u8> {
    unsafe { picker_vks().get(combo_sel(hwnd, ID_KEYS_COMBO)).copied() }
}

pub(super) unsafe fn after_change(hwnd: HWND, vk: u8, written: bool) {
    unsafe {
        invalidate();
        refresh(hwnd, true);
        select_key(hwnd, vk, true);
        if !written {
            let s = strings(current_lang());
            msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
    }
}

/// "Apply": give the picked key the window in the edit box.
pub(crate) unsafe fn apply_override(hwnd: HWND) {
    unsafe {
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
}

/// "Default": the picked key follows the global window again.
pub(crate) unsafe fn clear_override(hwnd: HWND) {
    unsafe {
        let Some(vk) = picked_vk(hwnd) else {
            return;
        };
        let written = settings_form::set_key_override(vk, None);
        after_change(hwnd, vk, written);
    }
}

/// "Clear stats": forget the picked key's counters and measured gaps.
pub(crate) unsafe fn clear_key_stats(hwnd: HWND) {
    unsafe {
        let Some(vk) = picked_vk(hwnd) else {
            return;
        };
        if let Some(shared) = SHARED.get() {
            shared.lock().unwrap().debouncer.clear_key_stats(vk);
        }
        // The day history is per-key statistics too: a replaced switch
        // must not inherit the old switch's trend.
        if let Some(saved) = SAVED.get() {
            let mut saved = saved.lock().unwrap();
            for entry in saved.day_history.iter_mut() {
                let i = vk as usize;
                entry.presses[i] = 0;
                entry.blocked[i] = 0;
            }
            for (_, vks) in saved.alerted.iter_mut() {
                vks.retain(|&k| k != vk);
            }
        }
        let written = settings_form::persist_live();
        after_change(hwnd, vk, written);
    }
}

/// "Apply suggestions": every key's suggested window at once, after a
/// confirmation listing the changes.
pub(crate) unsafe fn apply_all_suggestions(hwnd: HWND) {
    unsafe {
        let Some((_, snap)) = take_snapshot() else {
            return;
        };
        let s = strings(current_lang());
        let changes = suggested_changes(&snap);
        if changes.is_empty() {
            msg_box(hwnd, s.title, s.apply_all_none, MB_OK | MB_ICONINFORMATION);
            return;
        }
        let answer = msg_box(
            hwnd,
            s.key_apply_all_btn,
            &changes_text(s, &changes),
            MB_YESNO | MB_ICONQUESTION,
        );
        if answer != IDYES {
            return;
        }
        let set: Vec<(u8, Option<u64>)> =
            changes.iter().map(|&(vk, _, ms)| (vk, Some(ms))).collect();
        let written = settings_form::set_key_overrides(&set);
        invalidate();
        refresh(hwnd, true);
        let selected = STATE.lock().unwrap().selected;
        if let Some(vk) = selected {
            select_key(hwnd, vk, true);
        }
        if !written {
            msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
    }
}

/// Mouse "Apply": the window of every button without its own.
pub(crate) unsafe fn apply_mouse_threshold(hwnd: HWND) {
    unsafe {
        let Ok(ms) = get_text(hwnd, ID_MOUSE_MS_EDIT).parse::<u64>() else {
            let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONWARNING);
            return;
        };
        let (ms, written) = settings_form::set_mouse_threshold(ms);
        // Show the value in use (clamped to 1–1000).
        set_text(hwnd, ID_MOUSE_MS_EDIT, &ms.to_string());
        invalidate();
        refresh(hwnd, true);
        if !written {
            let s = strings(current_lang());
            msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
    }
}

/// Enter on the Keys tab applies the field it was pressed in.
pub(crate) unsafe fn on_enter(hwnd: HWND) {
    unsafe {
        let in_mouse_field = GetDlgItem(Some(hwnd), ID_MOUSE_MS_EDIT)
            .is_ok_and(|edit| windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() == edit);
        if in_mouse_field {
            apply_mouse_threshold(hwnd);
        } else {
            apply_override(hwnd);
        }
    }
}

pub(crate) unsafe fn on_mouse_toggled(hwnd: HWND, on: bool) {
    unsafe {
        if !settings_form::set_mouse_filter(on) {
            let s = strings(current_lang());
            msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
    }
}

pub(crate) unsafe fn on_wheel_toggled(hwnd: HWND, on: bool) {
    unsafe {
        if !settings_form::set_wheel_filter(on) {
            let s = strings(current_lang());
            msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        }
    }
}

/// `WM_NOTIFY` from the list: cell text, row color, and user selection.
pub(crate) unsafe fn on_notify(hwnd: HWND, lparam: LPARAM) -> Option<LRESULT> {
    unsafe {
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
}
