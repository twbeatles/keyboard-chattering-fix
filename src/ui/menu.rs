//! Window menu bar (File / Tools / Help).
//!
//! Rebuilt on every language change; check and enable states are applied
//! lazily in `WM_INITMENUPOPUP`, so the bar never goes stale.

use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::PCWSTR,
};

use super::{
    elevation, foreground::self_elevated, hotkeys, ids::*, state::current_lang, text::to_wide,
    tray::with_hotkey,
};
use crate::{app::state::SHARED, i18n::strings};

unsafe fn item(menu: HMENU, id: usize, text: &str) {
    unsafe {
        let v = to_wide(text);
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(v.as_ptr()));
    }
}

unsafe fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

unsafe fn popup(bar: HMENU, title: &str, sub: HMENU) {
    unsafe {
        let v = to_wide(title);
        let _ = AppendMenuW(bar, MF_POPUP, sub.0 as usize, PCWSTR(v.as_ptr()));
    }
}

/// Build the localized menu bar and attach it, destroying the previous one.
pub(crate) unsafe fn install(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let (Ok(bar), Ok(file), Ok(tools), Ok(help)) = (
            CreateMenu(),
            CreatePopupMenu(),
            CreatePopupMenu(),
            CreatePopupMenu(),
        ) else {
            return;
        };

        item(file, MENU_EXPORT_HISTORY, s.menu_export_history);
        item(file, MENU_EXPORT_SETTINGS, s.menu_export_settings);
        item(file, MENU_IMPORT_SETTINGS, s.menu_import_settings);
        separator(file);
        item(file, MENU_HIDE, s.menu_hide);
        separator(file);
        item(file, MENU_EXIT, s.menu_exit);

        item(
            tools,
            MENU_TOGGLE,
            &with_hotkey(s.menu_protect, hotkeys::toggle_label()),
        );
        item(
            tools,
            MENU_CLEANING,
            &with_hotkey(s.menu_cleaning, hotkeys::cleaning_label()),
        );
        separator(tools);
        item(tools, MENU_RESET_STATS, &format!("{}…", s.reset_stats_btn));
        item(tools, MENU_CLEAR_LOG, s.menu_clear_log);
        separator(tools);
        item(tools, MENU_OPEN_CPL, s.open_cpl_btn);
        item(tools, MENU_OPEN_CONFIG, s.menu_open_config);
        separator(tools);
        if self_elevated() {
            item(tools, MENU_ELEVATED_AUTOSTART, s.menu_elevated_autostart);
        } else {
            item(tools, MENU_RUN_ELEVATED, s.menu_run_elevated);
        }

        item(help, MENU_UPDATE, s.menu_update);
        item(help, MENU_HOMEPAGE, s.menu_homepage);
        separator(help);
        item(help, MENU_ABOUT, s.menu_about);

        popup(bar, s.menu_file, file);
        popup(bar, s.menu_tools, tools);
        popup(bar, s.menu_help, help);

        let old = GetMenu(hwnd);
        let _ = SetMenu(hwnd, Some(bar));
        if !old.is_invalid() {
            let _ = DestroyMenu(old);
        }
        let _ = DrawMenuBar(hwnd);
    }
}

/// `WM_INITMENUPOPUP`: reflect live state right before a menu opens.
pub(crate) unsafe fn on_init_menu_popup(hwnd: HWND) {
    unsafe {
        let bar = GetMenu(hwnd);
        if bar.is_invalid() {
            return;
        }
        let enabled = SHARED.get().is_none_or(|g| g.lock().unwrap().enabled);
        let _ = CheckMenuItem(
            bar,
            MENU_TOGGLE as u32,
            (MF_BYCOMMAND | if enabled { MF_CHECKED } else { MF_UNCHECKED }).0,
        );
        let _ = CheckMenuItem(
            bar,
            MENU_ELEVATED_AUTOSTART as u32,
            (MF_BYCOMMAND
                | if elevation::task_on() {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                })
            .0,
        );
        let _ = EnableMenuItem(
            bar,
            MENU_ELEVATED_AUTOSTART as u32,
            MF_BYCOMMAND
                | if elevation::task_busy() {
                    MF_GRAYED
                } else {
                    MF_ENABLED
                },
        );
        let busy = crate::updater::update_in_progress();
        let _ = EnableMenuItem(
            bar,
            MENU_UPDATE as u32,
            MF_BYCOMMAND | if busy { MF_GRAYED } else { MF_ENABLED },
        );
    }
}
