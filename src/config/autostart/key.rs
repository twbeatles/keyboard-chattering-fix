//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SAM_FLAGS, REG_VALUE_TYPE, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        },
    },
    core::PCWSTR,
};

pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An open registry key, closed on drop.
pub(super) struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl Key {
    pub(super) fn open(path: &str, access: REG_SAM_FLAGS) -> Option<Key> {
        let path = wide(path);
        let mut hkey = HKEY::default();
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                access,
                &mut hkey,
            ) == ERROR_SUCCESS
        }
        .then_some(Key(hkey))
    }

    pub(super) fn create(path: &str) -> Option<Key> {
        let path = wide(path);
        let mut hkey = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                Default::default(),
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            ) == ERROR_SUCCESS
        }
        .then_some(Key(hkey))
    }

    /// Read a value into `buf`; returns its type and length in bytes.
    pub(super) fn query(&self, name: &str, buf: &mut [u8]) -> Option<(REG_VALUE_TYPE, usize)> {
        let name = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = buf.len() as u32;
        let ok = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(buf.as_mut_ptr()),
                Some(&mut size),
            ) == ERROR_SUCCESS
        };
        ok.then_some((kind, (size as usize).min(buf.len())))
    }

    pub(super) fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> bool {
        let name = wide(name);
        unsafe {
            RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, kind, Some(data)) == ERROR_SUCCESS
        }
    }

    /// Delete a value; a missing value counts as deleted.
    pub(super) fn delete(&self, name: &str) -> bool {
        let name = wide(name);
        let r = unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())) };
        r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
    }
}

// ---- Run value -------------------------------------------------------------
