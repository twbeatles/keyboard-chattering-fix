//! Main window: control construction, painting, page switching, message
//! dispatch, and teardown.
//!
//! The procedure only routes messages; each domain lives in its own module:
//! `layout` describes the controls, `widgets` draws the owner-drawn ones,
//! `stats`/`history` refresh content, `settings_form` saves, `commands`
//! implements menu/tray/button actions, and `update` drives the
//! release-update flow.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};

use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::SystemServices::{SS_CENTER, SS_ENDELLIPSIS, SS_LEFT, SS_NOPREFIX},
        UI::{
            Controls::{
                InitCommonControlsEx, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVS_NOSORTHEADER,
                LVS_OWNERDATA, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL,
            },
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};

use super::{
    commands,
    controls::child,
    history,
    ids::*,
    layout::{
        cards, current_page, is_visible, sc, set_current_page, set_dpi, showing_cleaning, Kind,
        Page, CLIENT_H, CLIENT_W, SPECS, STAT_DIVIDERS, TAB_TRACK,
    },
    menu,
    paint::{
        apply_control_themes, apply_window_chrome, cached_brush, draw_rounded_card, font_for,
        scaled_rect, static_colors, theme_is_dark, ThemeColors,
    },
    settings_form::{self, on_save},
    state::{set_lang, SAVED},
    stats::update_stats,
    text::apply_language,
    tray::{add_tray, remove_tray, show_tray_menu},
    widgets::{draw_item, flip_toggle},
};
use crate::config::Settings;

/// `TaskbarCreated`: Explorer restarted and dropped every tray icon.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

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

unsafe fn create_controls(hwnd: HWND, hinstance: HINSTANCE) {
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

/// Show the controls of the current page (or the cleaning overlay).
pub(crate) unsafe fn apply_visibility(hwnd: HWND) {
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
    if page == Page::History {
        history::refresh(hwnd, true);
    }
}

unsafe fn switch_page(hwnd: HWND, page: Page) {
    if page == current_page() {
        return;
    }
    set_current_page(page);
    apply_visibility(hwnd);
}

unsafe fn on_create(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let cs = &*(lparam.0 as *const CREATESTRUCTW);
    let init = &*(cs.lpCreateParams as *const Settings);
    set_lang(init.ui_language());
    TASKBAR_CREATED.store(
        RegisterWindowMessageW(w!("TaskbarCreated")),
        Ordering::Relaxed,
    );
    let _ = SAVED.set(Mutex::new(init.clone()));

    apply_window_chrome(hwnd);
    create_controls(hwnd, cs.hInstance);
    history::init(hwnd);
    apply_control_themes(hwnd);
    // Fills combos and the menu, then the form loads the saved values.
    apply_language(hwnd);
    settings_form::load_into_form(hwnd, init);
    apply_visibility(hwnd);
    add_tray(hwnd);
    // Like a dialog: no dotted focus box on the tab until the keyboard is
    // used (Windows clears this itself on the first Tab/arrow press).
    let _ = SendMessageW(
        hwnd,
        WM_CHANGEUISTATE,
        Some(WPARAM(
            (UIS_SET as usize) | ((UISF_HIDEFOCUS as usize) << 16),
        )),
        Some(LPARAM(0)),
    );
    let _ = SetTimer(Some(hwnd), TIMER_STATS, 500, None);
    LRESULT(0)
}

unsafe fn on_paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (width, height) = (rc.right - rc.left, rc.bottom - rc.top);
    let colors = ThemeColors::new(theme_is_dark());

    // Double buffering.
    let mem_dc = CreateCompatibleDC(Some(hdc));
    let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
    let old_bmp = SelectObject(mem_dc, mem_bmp.into());

    FillRect(mem_dc, &rc, cached_brush(colors.bg));
    let cleaning = showing_cleaning();
    let page = current_page();
    if !cleaning {
        draw_rounded_card(
            mem_dc,
            &scaled_rect(TAB_TRACK),
            colors.tab_track,
            colors.tab_track,
            sc(10),
        );
    }
    for &card in cards(page, cleaning) {
        draw_rounded_card(
            mem_dc,
            &scaled_rect(card),
            colors.card_bg,
            colors.card_border,
            sc(12),
        );
    }
    if page == Page::Home && !cleaning {
        let pen = CreatePen(PS_SOLID, 1, colors.card_border);
        let old_pen = SelectObject(mem_dc, pen.into());
        for (x, y0, y1) in STAT_DIVIDERS {
            let _ = MoveToEx(mem_dc, sc(x), sc(y0), None);
            let _ = LineTo(mem_dc, sc(x), sc(y1));
        }
        SelectObject(mem_dc, old_pen);
        let _ = DeleteObject(pen.into());
    }

    let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(mem_bmp.into());
    let _ = DeleteDC(mem_dc);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn on_ctl_color_static(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut _);
    let id = GetWindowLongW(HWND(lparam.0 as *mut _), GWL_ID);
    let colors = ThemeColors::new(theme_is_dark());
    let (fg, bg) = match super::layout::spec(id) {
        Some(spec) => static_colors(spec.kind, spec.on_card, &colors),
        None => (colors.text_primary, colors.card_bg),
    };
    let _ = SetTextColor(hdc, fg);
    let _ = SetBkColor(hdc, bg);
    let _ = SetBkMode(hdc, TRANSPARENT);
    LRESULT(cached_brush(bg).0 as isize)
}

unsafe fn on_command(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let id = (wparam.0 & 0xFFFF) as i32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    let clicked = code == BN_CLICKED || code == BN_DOUBLECLICKED;

    // Enter inside a field (IsDialogMessage sends IDOK): save on the
    // settings pages, otherwise ignore. IDCANCEL (Esc) is ignored.
    if id == IDOK.0 {
        if matches!(current_page(), Page::Settings | Page::Advanced) && !showing_cleaning() {
            on_save(hwnd);
        }
        return;
    }
    if id == IDCANCEL.0 {
        return;
    }
    // Menu bar / accelerator commands carry no control handle.
    if lparam.0 == 0 {
        commands::run(hwnd, id as usize);
        return;
    }

    if let Some(page) = Page::from_tab_id(id) {
        if clicked {
            switch_page(hwnd, page);
        }
        return;
    }
    if let Some(spec) = super::layout::spec(id) {
        if spec.kind.is_toggle() && clicked {
            let on = flip_toggle(hwnd, id);
            if id == ID_TOGGLE_ENABLE {
                commands::set_protection(hwnd, on);
            } else {
                settings_form::mark_dirty(hwnd);
            }
            return;
        }
    }
    match (id, code) {
        (ID_PILL, BN_CLICKED | BN_DOUBLECLICKED) => commands::run(hwnd, MENU_TOGGLE),
        (ID_BTN_CLEAN_MODE, BN_CLICKED) => commands::run(hwnd, MENU_CLEANING),
        (ID_BTN_CLEAN_END, BN_CLICKED) => commands::set_cleaning(hwnd, false),
        (ID_BTN_RESET_STATS, BN_CLICKED) => commands::run(hwnd, MENU_RESET_STATS),
        (ID_BTN_OPEN_CPL, BN_CLICKED) => commands::run(hwnd, MENU_OPEN_CPL),
        (ID_BTN_CLEAR_LOG, BN_CLICKED) => commands::run(hwnd, MENU_CLEAR_LOG),
        (ID_BTN_SAVE, BN_CLICKED) => on_save(hwnd),
        (ID_BTN_SOUND_TEST, BN_CLICKED) => settings_form::preview_sound(hwnd),
        (ID_BTN_SOUND_BROWSE, BN_CLICKED) => settings_form::browse_sound(hwnd),
        (ID_HIST_SEARCH, EN_CHANGE) => history::on_search_changed(hwnd),
        (
            ID_EDIT_THRESHOLD | ID_EDIT_SOUND_FILE | ID_EDIT_MIN_CHATTER | ID_EDIT_LOG_LIMIT,
            EN_CHANGE,
        ) => settings_form::mark_dirty(hwnd),
        (ID_COMBO_MEASURE, CBN_SELCHANGE) => settings_form::mark_dirty(hwnd),
        (ID_COMBO_LANG, CBN_SELCHANGE) => settings_form::on_language_changed(hwnd),
        (ID_HIST_VIEW, CBN_SELCHANGE) => history::on_view_changed(hwnd),
        _ => {}
    }
}

unsafe fn on_destroy(hwnd: HWND) {
    let _ = KillTimer(Some(hwnd), TIMER_STATS);
    settings_form::persist_live();
    remove_tray(hwnd);
    // The hook thread is stopped by the bootstrap after the message loop.
    PostQuitMessage(0);
}

/// Dark menu bar. The classic menu bar is always painted white; Windows
/// offers owner-draw hooks for it through two private messages (the same
/// mechanism dark-mode Explorer, Notepad++ and others use). Only active in
/// dark mode; in light mode the system paints as usual.
mod dark_menu {
    use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_HOTLIGHT, ODS_SELECTED};

    use super::*;

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

    /// The 1 px line Windows draws under the menu bar would stay light.
    pub(super) unsafe fn paint_bottom_line(hwnd: HWND) {
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

    /// `Some(result)` when the message was handled.
    pub(super) unsafe fn handle(
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
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

/// How to answer the session-end messages.
#[derive(Debug, PartialEq, Eq)]
enum SessionReply {
    /// `WM_QUERYENDSESSION`: never veto a shutdown.
    AllowEnd,
    /// `WM_ENDSESSION(TRUE)`: the session really ends; persist now.
    PersistNow,
    /// `WM_ENDSESSION(FALSE)`: the shutdown was cancelled.
    Ignore,
}

fn session_reply(msg: u32, wparam: usize) -> SessionReply {
    match msg {
        WM_QUERYENDSESSION => SessionReply::AllowEnd,
        WM_ENDSESSION if wparam != 0 => SessionReply::PersistNow,
        _ => SessionReply::Ignore,
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => on_create(hwnd, lparam),
        WM_PAINT => {
            on_paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        0x0091 | 0x0092 => dark_menu::handle(hwnd, msg, wparam, lparam)
            .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
        WM_NCPAINT | WM_NCACTIVATE => {
            let result = DefWindowProcW(hwnd, msg, wparam, lparam);
            dark_menu::paint_bottom_line(hwnd);
            result
        }
        WM_CTLCOLORSTATIC => on_ctl_color_static(wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            let colors = ThemeColors::new(theme_is_dark());
            let _ = SetTextColor(hdc, colors.text_primary);
            let _ = SetBkColor(hdc, colors.edit_bg);
            LRESULT(cached_brush(colors.edit_bg).0 as isize)
        }
        WM_CTLCOLORBTN => {
            let colors = ThemeColors::new(theme_is_dark());
            LRESULT(cached_brush(colors.card_bg).0 as isize)
        }
        WM_DRAWITEM => {
            if draw_item(lparam) {
                LRESULT(1)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_NOTIFY => {
            history::on_notify(lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
        }
        WM_SETTINGCHANGE => {
            apply_window_chrome(hwnd);
            apply_control_themes(hwnd);
            history::apply_theme(hwnd);
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_ERASE | RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME,
            );
            LRESULT(0)
        }
        WM_INITMENUPOPUP => {
            menu::on_init_menu_popup(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            on_command(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_TIMER => {
            update_stats(hwnd);
            history::refresh(hwnd, false);
            settings_form::tick(hwnd);
            settings_form::autosave_tick();
            LRESULT(0)
        }
        WM_QUERYENDSESSION | WM_ENDSESSION => match session_reply(msg, wparam.0) {
            SessionReply::AllowEnd => LRESULT(1),
            SessionReply::PersistNow => {
                // Windows may terminate the process after this returns,
                // without WM_DESTROY: flush now (ISSUE-004).
                settings_form::persist_live();
                remove_tray(hwnd);
                LRESULT(0)
            }
            SessionReply::Ignore => LRESULT(0),
        },
        WM_CLOSE => {
            let close_to_tray = SAVED
                .get()
                .map(|s| s.lock().unwrap().close_to_tray)
                .unwrap_or(true);
            if close_to_tray {
                commands::hide_to_tray(hwnd);
            } else {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            on_destroy(hwnd);
            LRESULT(0)
        }
        _ if msg == WM_UPDATE_EXIT => {
            // Update helper launched: exit so the executable can be replaced.
            // WM_DESTROY persists counters and removes the tray icon.
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ if msg == WM_TRAY => match lparam.0 as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                commands::show_window(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                show_tray_menu(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        },
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            add_tray(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn system_dpi() -> i32 {
    let screen = GetDC(None);
    let dpi = GetDeviceCaps(Some(screen), LOGPIXELSY);
    ReleaseDC(None, screen);
    if dpi > 0 {
        dpi
    } else {
        96
    }
}

pub(crate) unsafe fn create_window(hinstance: HINSTANCE, init: &Settings) -> HWND {
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

pub(crate) unsafe fn show_initial(hwnd: HWND, minimized: bool) {
    let _ = ShowWindow(hwnd, if minimized { SW_HIDE } else { SW_SHOW });
    let _ = UpdateWindow(hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_end_persists_only_when_the_session_really_ends() {
        assert_eq!(session_reply(WM_QUERYENDSESSION, 0), SessionReply::AllowEnd);
        assert_eq!(session_reply(WM_ENDSESSION, 1), SessionReply::PersistNow);
        assert_eq!(session_reply(WM_ENDSESSION, 0), SessionReply::Ignore);
    }
}
