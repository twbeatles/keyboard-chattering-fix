//! System-tray icon, its tooltip/balloon, and its context menu.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM},
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
                NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::PCWSTR,
};

use super::{commands, hotkeys, ids::*, state::current_lang, text::to_wide};
use crate::i18n::strings;

/// What the tray icon and tooltip show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayState {
    On = 0,
    Paused = 1,
    Off = 2,
}

static STATE: AtomicU8 = AtomicU8::new(TrayState::On as u8);

fn state() -> TrayState {
    match STATE.load(Ordering::Relaxed) {
        1 => TrayState::Paused,
        2 => TrayState::Off,
        _ => TrayState::On,
    }
}

/// The "still running in the tray" balloon is shown once per session.
static BALLOON_SHOWN: AtomicBool = AtomicBool::new(false);

fn copy_wide(dst: &mut [u16], text: &str) {
    let v = to_wide(text);
    let n = v.len().min(dst.len());
    dst[..n].copy_from_slice(&v[..n]);
    // Truncated text must still be NUL-terminated.
    if n == dst.len()
        && let Some(last) = dst.last_mut()
    {
        *last = 0;
    }
}

fn base(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

fn current_tip() -> &'static str {
    let s = strings(current_lang());
    match state() {
        TrayState::On => s.tray_tip_on,
        TrayState::Paused => s.tray_tip_paused,
        TrayState::Off => s.tray_tip_off,
    }
}

/// Icon resource for the state: 1 = normal, 2 = gray (paused or off).
unsafe fn icon_for(st: TrayState) -> HICON {
    unsafe {
        let hinstance = HINSTANCE(
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .unwrap_or_default()
                .0,
        );
        let id: usize = if st == TrayState::On { 1 } else { 2 };
        // MAKEINTRESOURCE(id): a resource ID, not a real pointer.
        #[allow(clippy::manual_dangling_ptr)]
        LoadIconW(Some(hinstance), PCWSTR(id as *const u16))
            .or_else(|_| LoadIconW(Some(hinstance), PCWSTR(1 as *const u16)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

/// Add the icon. Also called again when Explorer restarts (TaskbarCreated).
pub(crate) unsafe fn add_tray(hwnd: HWND) {
    unsafe {
        let icon = icon_for(state());
        let mut nid = NOTIFYICONDATAW {
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..base(hwnd)
        };
        copy_wide(&mut nid.szTip, current_tip());
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

pub(crate) unsafe fn remove_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &base(hwnd));
    }
}

/// Keep the tooltip in sync with the language.
pub(crate) unsafe fn refresh_tip(hwnd: HWND) {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            uFlags: NIF_TIP,
            ..base(hwnd)
        };
        copy_wide(&mut nid.szTip, current_tip());
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// Show the protection state: gray icon while paused or off, and the
/// matching tooltip.
pub(crate) unsafe fn set_state(hwnd: HWND, st: TrayState) {
    unsafe {
        STATE.store(st as u8, Ordering::Relaxed);
        let mut nid = NOTIFYICONDATAW {
            uFlags: NIF_TIP | NIF_ICON,
            hIcon: icon_for(st),
            ..base(hwnd)
        };
        copy_wide(&mut nid.szTip, current_tip());
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// A one-off notification balloon from the tray icon.
pub(crate) unsafe fn balloon(hwnd: HWND, text: &str) {
    unsafe {
        let s = strings(current_lang());
        let mut nid = NOTIFYICONDATAW {
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_INFO,
            ..base(hwnd)
        };
        copy_wide(&mut nid.szInfoTitle, s.title);
        copy_wide(&mut nid.szInfo, text);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// First time the window goes to the tray, say where it went.
pub(crate) unsafe fn notify_hidden_once(hwnd: HWND) {
    unsafe {
        if BALLOON_SHOWN.swap(true, Ordering::Relaxed) {
            return;
        }
        let s = strings(current_lang());
        let mut nid = NOTIFYICONDATAW {
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_INFO,
            ..base(hwnd)
        };
        copy_wide(&mut nid.szInfoTitle, s.title);
        copy_wide(&mut nid.szInfo, s.tray_balloon);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// Menu text with its hotkey right-aligned (`Text\tCtrl+Alt+F12`).
pub(crate) fn with_hotkey(text: &str, hotkey: Option<String>) -> String {
    match hotkey {
        Some(hk) => format!("{text}\t{hk}"),
        None => text.to_string(),
    }
}

pub(crate) unsafe fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let item = |id: usize, text: &str, flags: MENU_ITEM_FLAGS| {
            let v = to_wide(text);
            let _ = AppendMenuW(menu, MF_STRING | flags, id, PCWSTR(v.as_ptr()));
        };
        let sep = || {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        };
        item(MENU_SHOW, s.menu_show, MF_ENABLED);
        sep();
        let toggle = if state() == TrayState::Off {
            s.menu_enable
        } else {
            s.menu_disable
        };
        item(
            MENU_TOGGLE,
            &with_hotkey(toggle, hotkeys::toggle_label()),
            MF_ENABLED,
        );
        item(
            MENU_CLEANING,
            &with_hotkey(s.menu_cleaning, hotkeys::cleaning_label()),
            MF_ENABLED,
        );
        sep();
        let busy = crate::updater::update_in_progress();
        item(
            MENU_UPDATE,
            s.menu_update,
            if busy { MF_GRAYED } else { MF_ENABLED },
        );
        sep();
        item(MENU_EXIT, s.menu_exit, MF_ENABLED);
        let _ = SetMenuDefaultItem(menu, MENU_SHOW as u32, 0);

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
        // Required after a notification-area TrackPopupMenu: forces the task
        // switch, or the next menu may close instantly / stay open (UI-004).
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        if picked.0 != 0 {
            commands::run(hwnd, picked.0 as usize);
        }
    }
}
