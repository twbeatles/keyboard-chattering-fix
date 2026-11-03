//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use std::fs;

/// A settings file as text. Files edited by hand may have been saved in
/// the system code page (an older editor, a Korean path): those are
/// decoded as such instead of being rejected, and a UTF-8 BOM is dropped.
pub(super) fn read_text(path: &std::path::Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_string()),
        Err(_) => decode_system_code_page(bytes),
    }
}

pub(super) fn decode_system_code_page(bytes: &[u8]) -> Option<String> {
    use windows::Win32::Globalization::{
        CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar,
    };
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    unsafe {
        let len = MultiByteToWideChar(CP_ACP, flags, bytes, None);
        if len <= 0 {
            return None;
        }
        let mut wide = vec![0u16; len as usize];
        let written = MultiByteToWideChar(CP_ACP, flags, bytes, Some(&mut wide));
        (written > 0).then(|| String::from_utf16_lossy(&wide[..written as usize]))
    }
}
