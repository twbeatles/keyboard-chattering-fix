//! Windows startup integration (`HKCU\...\Run`).

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
        },
    },
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "KeyboardChatteringFix";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Quote an executable path for the `Run` value (ISSUE-002).
fn quote_run_path(exe: &str) -> String {
    format!("\"{exe}\"")
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let key = wide(RUN_KEY);
        let value = wide(RUN_VALUE);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let mut size = 0u32;
        let present = RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            None,
            None,
            Some(&mut size),
        ) == ERROR_SUCCESS
            && size > 0;
        let _ = RegCloseKey(hkey);
        present
    }
}

pub fn set_autostart(enable: bool) -> bool {
    unsafe {
        let key = wide(RUN_KEY);
        let value = wide(RUN_VALUE);
        if !enable {
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                None,
                KEY_WRITE,
                &mut hkey,
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let r = RegDeleteValueW(hkey, PCWSTR(value.as_ptr()));
            let _ = RegCloseKey(hkey);
            // Deleting a missing value is still the desired end state.
            return r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND;
        }
        let mut hkey = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            None,
            PCWSTR::null(),
            Default::default(),
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let exe = match std::env::current_exe() {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(_) => return false,
        };
        // Quote so install locations containing spaces keep launching
        // (ISSUE-002). The NUL terminator is included in the byte count.
        let quoted = quote_run_path(&exe);
        let data: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: &[u8] = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let r = RegSetValueExW(hkey, PCWSTR(value.as_ptr()), Some(0), REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        r == ERROR_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_path_is_quoted() {
        assert_eq!(
            quote_run_path(r"C:\Program Files\App\app.exe"),
            r#""C:\Program Files\App\app.exe""#
        );
        assert_eq!(quote_run_path(r"C:\app.exe"), r#""C:\app.exe""#);
    }
}
