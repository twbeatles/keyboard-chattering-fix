//! Modal dialogs owned by the main window.

use windows::{
    core::PCWSTR,
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
};

use super::text::to_wide;

pub(crate) unsafe fn msg_box(
    hwnd: HWND,
    title: &str,
    text: &str,
    flags: MESSAGEBOX_STYLE,
) -> MESSAGEBOX_RESULT {
    let t = to_wide(title);
    let m = to_wide(text);
    MessageBoxW(Some(hwnd), PCWSTR(m.as_ptr()), PCWSTR(t.as_ptr()), flags)
}
