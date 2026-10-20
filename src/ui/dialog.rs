//! Modal dialogs owned by the main window.

use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::PCWSTR,
};

use super::{state::current_lang, text::to_wide};
use crate::{
    config,
    i18n::{fill, strings},
};

pub(crate) unsafe fn msg_box(
    hwnd: HWND,
    title: &str,
    text: &str,
    flags: MESSAGEBOX_STYLE,
) -> MESSAGEBOX_RESULT {
    unsafe {
        let t = to_wide(title);
        let m = to_wide(text);
        MessageBoxW(Some(hwnd), PCWSTR(m.as_ptr()), PCWSTR(t.as_ptr()), flags)
    }
}

/// Help > About: version, what the program does, where settings live.
pub(crate) unsafe fn show_about(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let text = fill(
            s.about_fmt,
            &[
                config::VERSION,
                &config::config_path().display().to_string(),
                config::HOMEPAGE_URL,
            ],
        );
        let title = s.menu_about.replace('&', "");
        msg_box(hwnd, &title, &text, MB_OK | MB_ICONINFORMATION);
    }
}

/// Startup warning: the keyboard hook could not be installed (G1).
pub(crate) unsafe fn show_hook_failed(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        msg_box(hwnd, s.title, s.hook_failed, MB_OK | MB_ICONWARNING);
    }
}
