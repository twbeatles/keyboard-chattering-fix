use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};

use super::super::{
    commands, foreground, history,
    ids::*,
    keys_page,
    layout::Page,
    menu,
    paint::{ThemeColors, apply_control_themes, apply_window_chrome, cached_brush, theme_is_dark},
    settings_form::{self},
    state::SAVED,
    stats::update_stats,
    tray::{add_tray, remove_tray, show_tray_menu},
    widgets::draw_item,
};

use super::create::switch_page;
use super::dark_menu;
use super::handlers::{on_command, on_create, on_ctl_color_static, on_destroy, on_paint};
use super::nonce::update_exit_authenticated;
use super::session::{SessionReply, session_reply};

/// `TaskbarCreated`: Explorer restarted and dropped every tray icon.
pub(super) static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
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
                if let Some(vk) = history::double_clicked_vk(lparam) {
                    // Per-key summary row: change that key's window.
                    switch_page(hwnd, Page::Keys);
                    keys_page::select_key(hwnd, vk, true);
                    return LRESULT(0);
                }
                history::on_notify(lparam)
                    .or_else(|| keys_page::on_notify(hwnd, lparam))
                    .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }
            WM_SETTINGCHANGE => {
                apply_window_chrome(hwnd);
                apply_control_themes(hwnd);
                history::apply_theme(hwnd);
                keys_page::apply_theme(hwnd);
                let _ = RedrawWindow(
                    Some(hwnd),
                    None,
                    None,
                    RDW_ERASE | RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME,
                );
                LRESULT(0)
            }
            WM_HOTKEY => {
                match wparam.0 as i32 {
                    HOTKEY_TOGGLE => commands::run(hwnd, MENU_TOGGLE),
                    HOTKEY_CLEANING => commands::run(hwnd, MENU_CLEANING),
                    _ => {}
                }
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
                commands::cleaning_tick(hwnd);
                commands::pause_tick(hwnd);
                if foreground::tick(hwnd) {
                    super::super::stats::invalidate();
                }
                if super::super::watchdog::tick() {
                    super::super::stats::invalidate();
                }
                update_stats(hwnd);
                history::refresh(hwnd, false);
                keys_page::refresh(hwnd, false);
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
                // Anyone in the session can post the constant id: only the
                // holder of this process's nonce exits it (SEC-001).
                if !update_exit_authenticated(wparam.0) {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                // Update helper launched (or elevated copy approved): exit so the
                // executable can be replaced. WM_DESTROY persists counters and
                // removes the tray icon.
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            _ if msg == WM_ELEVATED_TASK_DONE => {
                super::super::elevation::on_task_done(hwnd);
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
}
