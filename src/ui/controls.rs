//! Win32 control factory used while building the main window.

use windows::{
    Win32::{Foundation::*, Graphics::Gdi::HFONT, UI::WindowsAndMessaging::*},
    core::PCWSTR,
};

use super::text::to_wide;

// Win32 control factory: the parameter list mirrors `CreateWindowExW`, so
// splitting it would only add indirection at every call site.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w_: i32,
    h: i32,
    id: i32,
    hinstance: HINSTANCE,
    font: HFONT,
) -> HWND {
    unsafe {
        let t = to_wide(text);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR(t.as_ptr()),
            style | WS_CHILD | WS_VISIBLE,
            x,
            y,
            w_,
            h,
            Some(parent),
            Some(HMENU(id as usize as *mut _)),
            Some(hinstance),
            None,
        )
        .unwrap_or_default();

        if !hwnd.is_invalid() && !font.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        hwnd
    }
}
