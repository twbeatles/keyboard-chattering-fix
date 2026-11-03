/// Dark menu bar. The classic menu bar is always painted white; Windows
/// offers owner-draw hooks for it through two private messages (the same
/// mechanism dark-mode Explorer, Notepad++ and others use). Only active in
/// dark mode; in light mode the system paints as usual.
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        DT_CENTER, DT_HIDEPREFIX, DT_SINGLELINE, DT_VCENTER, DrawTextW, FillRect, GetWindowDC, HDC,
        ReleaseDC, SetBkMode, SetTextColor, TRANSPARENT,
    },
    UI::{
        Controls::{DRAWITEMSTRUCT, ODS_HOTLIGHT, ODS_SELECTED},
        WindowsAndMessaging::{
            GetMenuBarInfo, GetMenuItemInfoW, GetWindowRect, HMENU, MENUBARINFO, MENUITEMINFOW,
            MIIM_STRING, OBJID_MENU,
        },
    },
};
use windows::core::PWSTR;

use super::super::paint::{ThemeColors, cached_brush, theme_is_dark};

const WM_UAHDRAWMENU: u32 = 0x0091;
const WM_UAHDRAWMENUITEM: u32 = 0x0092;

/// Leading part of `UAHMENU` (`hmenu`, `hdc`, flags).
#[repr(C)]
struct UahMenu {
    hmenu: HMENU,
    hdc: HDC,
    flags: u32,
}

/// Leading part of `UAHDRAWMENUITEM`: the owner-draw item, the menu, and
/// the item's position. The metrics that follow are not needed.
#[repr(C)]
struct UahDrawMenuItem {
    dis: DRAWITEMSTRUCT,
    um: UahMenu,
    position: i32,
}

/// Menu bar rectangle in window coordinates (what the window DC uses).
unsafe fn bar_rect(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let mut mbi = MENUBARINFO {
            cbSize: std::mem::size_of::<MENUBARINFO>() as u32,
            ..Default::default()
        };
        GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut mbi).ok()?;
        let mut win = RECT::default();
        GetWindowRect(hwnd, &mut win).ok()?;
        let mut rc = mbi.rcBar;
        rc.left -= win.left;
        rc.right -= win.left;
        rc.top -= win.top;
        rc.bottom -= win.top;
        Some(rc)
    }
}

/// The 1 px line Windows draws under the menu bar would stay light.
pub(super) unsafe fn paint_bottom_line(hwnd: HWND) {
    unsafe {
        if !theme_is_dark() {
            return;
        }
        let Some(bar) = bar_rect(hwnd) else {
            return;
        };
        let line = RECT {
            top: bar.bottom,
            bottom: bar.bottom + 1,
            ..bar
        };
        let hdc = GetWindowDC(Some(hwnd));
        FillRect(hdc, &line, cached_brush(ThemeColors::new(true).bg));
        ReleaseDC(Some(hwnd), hdc);
    }
}

/// `Some(result)` when the message was handled.
pub(super) unsafe fn handle(
    hwnd: HWND,
    msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    unsafe {
        if !theme_is_dark() {
            return None;
        }
        let c = ThemeColors::new(true);
        match msg {
            WM_UAHDRAWMENU => {
                let um = &*(lparam.0 as *const UahMenu);
                let rc = bar_rect(hwnd)?;
                FillRect(um.hdc, &rc, cached_brush(c.bg));
                Some(LRESULT(1))
            }
            WM_UAHDRAWMENUITEM => {
                let item = &*(lparam.0 as *const UahDrawMenuItem);
                let state = item.dis.itemState.0;
                let hot = state & (ODS_HOTLIGHT.0 | ODS_SELECTED.0) != 0;
                let mut text = [0u16; 64];
                let mut info = MENUITEMINFOW {
                    cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_STRING,
                    dwTypeData: PWSTR(text.as_mut_ptr()),
                    cch: text.len() as u32 - 1,
                    ..Default::default()
                };
                let _ = GetMenuItemInfoW(item.um.hmenu, item.position as u32, true, &mut info);
                let len = (info.cch as usize).min(text.len() - 1);
                let mut rc = item.dis.rcItem;
                FillRect(
                    item.um.hdc,
                    &rc,
                    cached_brush(if hot { c.tab_selected } else { c.bg }),
                );
                let _ = SetBkMode(item.um.hdc, TRANSPARENT);
                let _ = SetTextColor(item.um.hdc, c.text_primary);
                DrawTextW(
                    item.um.hdc,
                    &mut text[..len],
                    &mut rc,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_HIDEPREFIX,
                );
                Some(LRESULT(1))
            }
            _ => None,
        }
    }
}
