use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::*,
        System::SystemServices::{SS_CENTER, SS_ENDELLIPSIS, SS_LEFT, SS_NOPREFIX},
        UI::{
            Controls::{
                ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVS_NOSORTHEADER,
                LVS_OWNERDATA, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL,
            },
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

use super::super::{
    controls::child,
    history,
    ids::*,
    keys_page,
    layout::{
        CLIENT_H, CLIENT_W, Kind, Page, SPECS, current_page, is_visible, sc, set_current_page,
        set_dpi, showing_cleaning,
    },
    paint::font_for,
};
use crate::config::Settings;

use super::proc::wndproc;

unsafe fn style_for(kind: Kind) -> WINDOW_STYLE {
    let tab = WS_TABSTOP.0;
    WINDOW_STYLE(match kind {
        Kind::CleanTitle | Kind::CleanBody | Kind::Footer => SS_CENTER.0 | SS_NOPREFIX.0,
        Kind::Hint | Kind::Subtitle => SS_LEFT.0 | SS_NOPREFIX.0,
        k if k.is_static() => SS_LEFT.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0,
        Kind::Edit => WS_BORDER.0 | tab | ES_AUTOHSCROLL as u32,
        Kind::NumberEdit => WS_BORDER.0 | tab | ES_NUMBER as u32 | ES_CENTER as u32,
        Kind::Combo => CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | tab,
        Kind::List => {
            WS_BORDER.0
                | tab
                | LVS_REPORT
                | LVS_OWNERDATA
                | LVS_SINGLESEL
                | LVS_SHOWSELALWAYS
                | LVS_NOSORTHEADER
        }
        k if k.is_owner_drawn() => BS_OWNERDRAW as u32 | tab,
        _ => 0,
    })
}

unsafe fn class_for(kind: Kind) -> PCWSTR {
    match kind {
        k if k.is_static() => w!("STATIC"),
        Kind::Edit | Kind::NumberEdit => w!("EDIT"),
        Kind::Combo => w!("COMBOBOX"),
        Kind::List => w!("SysListView32"),
        _ => w!("BUTTON"),
    }
}

pub(super) unsafe fn create_controls(hwnd: HWND, hinstance: HINSTANCE) {
    unsafe {
        for spec in SPECS {
            let (x, y, w_, h) = spec.rect;
            child(
                hwnd,
                class_for(spec.kind),
                "",
                style_for(spec.kind),
                sc(x),
                sc(y),
                sc(w_),
                sc(h),
                spec.id,
                hinstance,
                font_for(spec.kind),
            );
        }
    }
}

/// Show the controls of the current page (or the cleaning overlay).
pub(crate) unsafe fn apply_visibility(hwnd: HWND) {
    unsafe {
        let page = current_page();
        let cleaning = showing_cleaning();
        let _ = SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(0)), Some(LPARAM(0)));
        for spec in SPECS {
            if let Ok(h) = GetDlgItem(Some(hwnd), spec.id) {
                let show = is_visible(spec.scope, page, cleaning);
                let _ = ShowWindow(h, if show { SW_SHOWNA } else { SW_HIDE });
            }
        }
        let _ = SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(1)), Some(LPARAM(0)));
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_ERASE | RDW_INVALIDATE | RDW_ALLCHILDREN,
        );

        // Never leave keyboard focus on a control that just disappeared.
        let focus = GetFocus();
        let focus_hidden = focus.is_invalid() || !IsWindowVisible(focus).as_bool();
        if focus_hidden && IsWindowVisible(hwnd).as_bool() {
            let target = if cleaning {
                ID_BTN_CLEAN_END
            } else {
                page.tab_id()
            };
            if let Ok(h) = GetDlgItem(Some(hwnd), target) {
                let _ = SetFocus(Some(h));
            }
        }
        match page {
            Page::History => history::refresh(hwnd, true),
            Page::Keys => keys_page::refresh(hwnd, true),
            _ => {}
        }
    }
}

pub(super) unsafe fn switch_page(hwnd: HWND, page: Page) {
    unsafe {
        if page == current_page() {
            return;
        }
        set_current_page(page);
        apply_visibility(hwnd);
    }
}

unsafe fn system_dpi() -> i32 {
    unsafe {
        let screen = GetDC(None);
        let dpi = GetDeviceCaps(Some(screen), LOGPIXELSY);
        ReleaseDC(None, screen);
        if dpi > 0 { dpi } else { 96 }
    }
}

pub(crate) unsafe fn create_window(hinstance: HINSTANCE, init: &Settings) -> HWND {
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
        set_dpi(system_dpi());

        let class = w!("KeyboardChatteringFixMain");
        // MAKEINTRESOURCE(1): see add_tray for why the lint is suppressed here.
        #[allow(clippy::manual_dangling_ptr)]
        let icon = LoadIconW(Some(hinstance), PCWSTR(1 as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Outer size from the logical client size, including the menu bar.
        // WS_CLIPCHILDREN: the double-buffered BitBlt in WM_PAINT must never
        // paint over child controls that already drew themselves.
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: sc(CLIENT_W),
            bottom: sc(CLIENT_H),
        };
        let _ = AdjustWindowRectEx(&mut outer, style, true, WINDOW_EX_STYLE::default());

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Keyboard Chatter Fix"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            outer.right - outer.left,
            outer.bottom - outer.top,
            Some(HWND::default()),
            Some(HMENU::default()),
            Some(hinstance),
            Some(init as *const Settings as *const _),
        )
        .unwrap_or_default();

        if !icon.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }

        hwnd
    }
}

pub(crate) unsafe fn show_initial(hwnd: HWND, minimized: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if minimized { SW_HIDE } else { SW_SHOW });
        let _ = UpdateWindow(hwnd);
    }
}
