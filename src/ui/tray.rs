//! System-tray icon and its context menu.

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HINSTANCE, HWND, POINT},
        Graphics::Gdi::InvalidateRect,
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::*,
        },
    },
};

use super::{
    ids::*,
    state::current_lang,
    stats::update_stats,
    text::{set_check, to_wide},
    update::spawn_update_check,
};
use crate::{app::state::SHARED, i18n::strings};

pub(crate) unsafe fn add_tray(hwnd: HWND) {
    let s = strings(current_lang());
    let hinstance = HINSTANCE(
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap_or_default()
            .0,
    );
    // MAKEINTRESOURCE(1): icon resource ID, not a real pointer. Clippy's
    // `dangling()` suggestion would change the address and break loading.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = LoadIconW(Some(hinstance), PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let tip = to_wide(s.tray_tip);
    let n = tip.len().min(nid.szTip.len());
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

pub(crate) unsafe fn remove_tray(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

pub(crate) unsafe fn show_tray_menu(hwnd: HWND) {
    let s = strings(current_lang());
    let enabled = SHARED
        .get()
        .map(|g| g.lock().unwrap().enabled)
        .unwrap_or(true);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let item = |id: usize, text: &str| {
        let v = to_wide(text);
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(v.as_ptr()));
    };
    item(MENU_SHOW, s.menu_show);
    item(
        MENU_TOGGLE,
        if enabled {
            s.menu_disable
        } else {
            s.menu_enable
        },
    );
    item(MENU_CLEANING, s.menu_cleaning);
    item(MENU_UPDATE, s.menu_update);
    item(MENU_EXIT, s.menu_exit);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    match picked.0 as usize {
        MENU_SHOW => {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        MENU_TOGGLE => {
            if let Some(g) = SHARED.get() {
                let mut g = g.lock().unwrap();
                g.enabled = !g.enabled;
                set_check(hwnd, ID_CHECK_ENABLE, g.enabled);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        MENU_CLEANING => {
            if let Some(g) = SHARED.get() {
                g.lock().unwrap().debouncer.cleaning_mode = true;
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            update_stats(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        MENU_UPDATE => {
            spawn_update_check(hwnd);
        }
        MENU_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}
