//! Owner-drawn controls: toggle switches, push buttons, tabs, status pill.
//!
//! Native themed checkboxes ignore `WM_CTLCOLORSTATIC` text colors, so in
//! dark mode their labels turn black on gray. Drawing these few controls
//! ourselves keeps both themes consistent and lets the toggles look like
//! Windows 11 switches. Toggle state lives here because `BS_OWNERDRAW`
//! buttons have no check state of their own.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{Controls::*, WindowsAndMessaging::*},
};

use super::{
    layout::{current_page, sc, spec, Kind, Page},
    paint::{cached_brush, theme_is_dark, ThemeColors},
};

static TOGGLES: Mutex<Vec<(i32, bool)>> = Mutex::new(Vec::new());
/// Protection state as last rendered, read by the pill painter so paint
/// paths never lock the hook's shared state.
static PROTECTION_ON: AtomicBool = AtomicBool::new(true);

pub(crate) fn get_toggle(id: i32) -> bool {
    TOGGLES
        .lock()
        .unwrap()
        .iter()
        .find(|(i, _)| *i == id)
        .is_some_and(|(_, on)| *on)
}

fn store_toggle(id: i32, on: bool) {
    let mut t = TOGGLES.lock().unwrap();
    match t.iter_mut().find(|(i, _)| *i == id) {
        Some(slot) => slot.1 = on,
        None => t.push((id, on)),
    }
}

pub(crate) unsafe fn set_toggle(parent: HWND, id: i32, on: bool) {
    store_toggle(id, on);
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        let _ = InvalidateRect(Some(h), None, false);
    }
}

/// Flip a toggle after a click and return its new state.
pub(crate) unsafe fn flip_toggle(parent: HWND, id: i32) -> bool {
    let on = !get_toggle(id);
    set_toggle(parent, id, on);
    on
}

pub(crate) fn protection_on() -> bool {
    PROTECTION_ON.load(Ordering::Relaxed)
}

pub(crate) fn set_protection_on(on: bool) {
    PROTECTION_ON.store(on, Ordering::Relaxed);
}

unsafe fn rounded(hdc: HDC, rc: &RECT, fill: COLORREF, border: COLORREF, radius: i32) {
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_pen = SelectObject(hdc, pen.into());
    let old_brush = SelectObject(hdc, cached_brush(fill).into());
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(pen.into());
}

unsafe fn ellipse(hdc: HDC, rc: &RECT, fill: COLORREF) {
    let pen = CreatePen(PS_SOLID, 1, fill);
    let old_pen = SelectObject(hdc, pen.into());
    let old_brush = SelectObject(hdc, cached_brush(fill).into());
    let _ = Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(pen.into());
}

unsafe fn control_text(h: HWND) -> Vec<u16> {
    let mut buf = [0u16; 256];
    let len = GetWindowTextW(h, &mut buf) as usize;
    buf[..len.min(buf.len())].to_vec()
}

unsafe fn draw_label(
    hdc: HDC,
    text: &mut [u16],
    rc: &mut RECT,
    color: COLORREF,
    flags: DRAW_TEXT_FORMAT,
) {
    let _ = SetTextColor(hdc, color);
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = DrawTextW(hdc, text, rc, flags | DT_NOPREFIX);
}

/// `WM_DRAWITEM` for every owner-drawn button.
pub(crate) unsafe fn draw_item(lparam: LPARAM) -> bool {
    let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
    if dis.CtlType != ODT_BUTTON {
        return false;
    }
    let Some(spec) = spec(dis.CtlID as i32) else {
        return false;
    };
    let c = ThemeColors::new(theme_is_dark());
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let state = dis.itemState.0;
    let pressed = state & ODS_SELECTED.0 != 0;
    let focused = state & ODS_FOCUS.0 != 0 && state & ODS_NOFOCUSRECT.0 == 0;
    let parent_bg = if spec.on_card { c.card_bg } else { c.bg };
    let mut text = control_text(dis.hwndItem);

    let font =
        HFONT(SendMessageW(dis.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    let old_font = SelectObject(hdc, font.into());

    match spec.kind {
        Kind::Toggle | Kind::ToggleLarge => {
            let _ = FillRect(hdc, &rc, cached_brush(parent_bg));
            let on = get_toggle(spec.id);
            let (tw, th) = (sc(40), sc(20));
            let top = rc.top + (rc.bottom - rc.top - th) / 2;
            let track = RECT {
                left: rc.left,
                top,
                right: rc.left + tw,
                bottom: top + th,
            };
            let knob_d = th - sc(8);
            let knob_left = if on {
                track.right - sc(4) - knob_d
            } else {
                track.left + sc(4)
            };
            let knob = RECT {
                left: knob_left,
                top: top + sc(4),
                right: knob_left + knob_d,
                bottom: top + sc(4) + knob_d,
            };
            if on {
                let fill = if pressed { c.accent_pressed } else { c.accent };
                rounded(hdc, &track, fill, fill, th);
                ellipse(hdc, &knob, c.on_accent);
            } else {
                rounded(hdc, &track, parent_bg, c.toggle_off, th);
                ellipse(hdc, &knob, c.toggle_off);
            }
            let mut label = RECT {
                left: track.right + sc(12),
                ..rc
            };
            draw_label(
                hdc,
                &mut text,
                &mut label,
                c.text_primary,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            if focused {
                let mut measure = label;
                let _ = DrawTextW(
                    hdc,
                    &mut text,
                    &mut measure,
                    DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
                );
                let fr = RECT {
                    left: label.left - sc(3),
                    top: rc.top + sc(2),
                    right: measure.right.min(rc.right) + sc(3),
                    bottom: rc.bottom - sc(2),
                };
                let _ = DrawFocusRect(hdc, &fr);
            }
        }
        Kind::Button | Kind::Primary => {
            let _ = FillRect(hdc, &rc, cached_brush(parent_bg));
            let (fill, border, fg) = if spec.kind == Kind::Primary {
                let f = if pressed { c.accent_pressed } else { c.accent };
                (f, f, c.on_accent)
            } else {
                let f = if pressed {
                    c.button_pressed
                } else {
                    c.button_bg
                };
                (f, c.button_border, c.text_primary)
            };
            rounded(hdc, &rc, fill, border, sc(8));
            let mut inner = rc;
            draw_label(
                hdc,
                &mut text,
                &mut inner,
                fg,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            if focused {
                let fr = RECT {
                    left: rc.left + sc(3),
                    top: rc.top + sc(3),
                    right: rc.right - sc(3),
                    bottom: rc.bottom - sc(3),
                };
                let _ = DrawFocusRect(hdc, &fr);
            }
        }
        Kind::Tab => {
            let _ = FillRect(hdc, &rc, cached_brush(c.tab_track));
            let selected = Page::from_tab_id(spec.id) == Some(current_page());
            let fg = if selected {
                let pill = RECT {
                    left: rc.left + sc(2),
                    top: rc.top + sc(1),
                    right: rc.right - sc(2),
                    bottom: rc.bottom - sc(1),
                };
                rounded(hdc, &pill, c.tab_selected, c.card_border, sc(8));
                c.accent
            } else {
                c.text_secondary
            };
            let mut inner = rc;
            draw_label(
                hdc,
                &mut text,
                &mut inner,
                fg,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if focused {
                let fr = RECT {
                    left: rc.left + sc(6),
                    top: rc.top + sc(4),
                    right: rc.right - sc(6),
                    bottom: rc.bottom - sc(4),
                };
                let _ = DrawFocusRect(hdc, &fr);
            }
        }
        Kind::Pill => {
            let _ = FillRect(hdc, &rc, cached_brush(parent_bg));
            let (fg, bg) = if PROTECTION_ON.load(Ordering::Relaxed) {
                (c.status_active_fg, c.status_active_bg)
            } else {
                (c.status_paused_fg, c.status_paused_bg)
            };
            let fill = if pressed { parent_bg } else { bg };
            rounded(hdc, &rc, fill, fg, rc.bottom - rc.top);
            let mut inner = rc;
            draw_label(
                hdc,
                &mut text,
                &mut inner,
                fg,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if focused {
                let fr = RECT {
                    left: rc.left + sc(8),
                    top: rc.top + sc(4),
                    right: rc.right - sc(8),
                    bottom: rc.bottom - sc(4),
                };
                let _ = DrawFocusRect(hdc, &fr);
            }
        }
        _ => {}
    }

    SelectObject(hdc, old_font);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_state_round_trips_per_id() {
        // IDs outside the real layout so parallel UI tests cannot collide.
        store_toggle(9001, true);
        store_toggle(9002, false);
        assert!(get_toggle(9001));
        assert!(!get_toggle(9002));
        store_toggle(9001, false);
        assert!(!get_toggle(9001));
        // Unknown controls read as off instead of panicking.
        assert!(!get_toggle(9999));
    }
}
