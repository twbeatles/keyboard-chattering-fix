//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`: when both fire, the second instance leaves
//! quietly instead of showing "already running".

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RegCloseKey, RegCreateKeyExW,
            RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        },
    },
    core::PCWSTR,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "KeyboardChatteringFix";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Command-line flag of automatic starts (see the module docs).
pub const AUTOSTART_ARG: &str = "--autostart";
/// Name of the scheduled task for the elevated start.
const TASK_NAME: &str = "KeyboardChatteringFix";

/// Quote an executable path for the `Run` value (ISSUE-002).
fn quote_run_path(exe: &str) -> String {
    format!("\"{exe}\" {AUTOSTART_ARG}")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Task definition: at `user`'s sign-in, run `exe` with highest privileges,
/// also on battery, with no time limit (the defaults of `schtasks /create`
/// would skip laptops on battery and stop the program after 72 hours).
pub fn task_xml(exe: &str, user: &str) -> String {
    let (exe, user) = (xml_escape(exe), xml_escape(user));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Keyboard Chatter Fix (administrator start)</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>{AUTOSTART_ARG}</Arguments></Exec></Actions>
</Task>
"#
    )
}

fn schtasks(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether the elevated sign-in task exists (spawns `schtasks`; call off
/// the UI thread).
pub fn elevated_task_exists() -> bool {
    schtasks(&["/query", "/tn", TASK_NAME])
}

/// Create or remove the elevated sign-in task. Creating it needs an
/// elevated process. Returns whether the change took effect.
pub fn set_elevated_task(enable: bool) -> bool {
    if !enable {
        return schtasks(&["/delete", "/tn", TASK_NAME, "/f"]) || !elevated_task_exists();
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) => format!(r"{d}\{u}"),
        (_, Ok(u)) => u,
        _ => return false,
    };
    // schtasks reads the definition as UTF-16 (with BOM).
    let xml = task_xml(&exe.to_string_lossy(), &user);
    let bytes: Vec<u8> = std::iter::once(0xFEFFu16)
        .chain(xml.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    let path = std::env::temp_dir().join(format!("kcfix_task_{}.xml", std::process::id()));
    if std::fs::write(&path, bytes).is_err() {
        return false;
    }
    let ok = schtasks(&[
        "/create",
        "/tn",
        TASK_NAME,
        "/xml",
        &path.to_string_lossy(),
        "/f",
    ]);
    let _ = std::fs::remove_file(&path);
    ok
}

/// The `Run` value as stored, if any.
fn run_value() -> Option<String> {
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
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let ok = RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        ) == ERROR_SUCCESS;
        let _ = RegCloseKey(hkey);
        if !ok {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        Some(text.trim_end_matches('\0').to_string())
    }
}

/// Whether a stored `Run` value starts *this* executable but predates the
/// `--autostart` flag. Entries for another copy are never touched.
fn needs_flag_upgrade(stored: &str, exe: &str) -> bool {
    let path = stored.trim().trim_start_matches('"');
    let path = path.split('"').next().unwrap_or(path).trim();
    path.eq_ignore_ascii_case(exe) && !stored.contains(AUTOSTART_ARG)
}

/// Add `--autostart` to an older startup entry of this executable.
pub fn upgrade_autostart_entry() {
    let (Some(stored), Ok(exe)) = (run_value(), std::env::current_exe()) else {
        return;
    };
    if needs_flag_upgrade(&stored, &exe.to_string_lossy()) {
        let _ = set_autostart(true);
    }
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
            r#""C:\Program Files\App\app.exe" --autostart"#
        );
        assert_eq!(quote_run_path(r"C:\app.exe"), r#""C:\app.exe" --autostart"#);
    }

    #[test]
    fn only_our_own_old_entry_gets_the_flag() {
        let exe = r"C:\Tools\KeyboardChatteringFix.exe";
        assert!(needs_flag_upgrade(
            r#""C:\Tools\KeyboardChatteringFix.exe""#,
            exe
        ));
        assert!(needs_flag_upgrade(
            r#""c:\tools\keyboardchatteringfix.exe""#,
            exe
        ));
        // Already upgraded.
        assert!(!needs_flag_upgrade(&quote_run_path(exe), exe));
        // Another copy (e.g. a test build) must never take over the entry.
        assert!(!needs_flag_upgrade(
            r#""D:\dev\target\debug\kcfix.exe""#,
            exe
        ));
    }

    #[test]
    fn task_definition_runs_elevated_on_battery_without_time_limit() {
        let xml = task_xml(r"C:\Tools & Co\kcfix.exe", r"PC\kim");
        assert!(xml.contains("<Command>C:\\Tools &amp; Co\\kcfix.exe</Command>"));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
        assert!(xml.contains("<UserId>PC\\kim</UserId>"));
        for needle in [
            "<RunLevel>HighestAvailable</RunLevel>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<LogonType>InteractiveToken</LogonType>",
        ] {
            assert!(xml.contains(needle), "{needle}");
        }
    }
}
