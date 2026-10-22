//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

use std::path::Path;

use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_SAM_FLAGS, REG_SZ,
                REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW,
                RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
            },
            TaskScheduler::{
                IExecAction, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE,
                TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
            },
            Threading::{
                GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            },
            Variant::VARIANT,
        },
        UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
    },
    core::{BSTR, Interface, PCWSTR, PWSTR},
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "KeyboardChatteringFix";
/// Task Manager's Startup tab (and Settings > Apps > Startup) disables an
/// entry without removing it: `StartupApproved\Run` holds a binary value of
/// the same name whose first byte is odd (`03`) when disabled.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Program state outside the settings file: only the elevated-task marker.
const STATE_KEY: &str = r"Software\KeyboardChatteringFix";
const MARKER_VALUE: &str = "ElevatedTask";

/// Command-line flag of automatic starts (see the module docs).
pub const AUTOSTART_ARG: &str = "--autostart";
/// Name of the scheduled task for the elevated start.
const TASK_NAME: &str = "KeyboardChatteringFix";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl Key {
    fn open(path: &str, access: REG_SAM_FLAGS) -> Option<Key> {
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

    fn create(path: &str) -> Option<Key> {
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
    fn query(&self, name: &str, buf: &mut [u8]) -> Option<(REG_VALUE_TYPE, usize)> {
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

    fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> bool {
        let name = wide(name);
        unsafe {
            RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, kind, Some(data)) == ERROR_SUCCESS
        }
    }

    /// Delete a value; a missing value counts as deleted.
    fn delete(&self, name: &str) -> bool {
        let name = wide(name);
        let r = unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())) };
        r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
    }
}

// ---- Run value -------------------------------------------------------------

/// Quote an executable path for the `Run` value (ISSUE-002).
fn quote_run_path(exe: &str) -> String {
    format!("\"{exe}\" {AUTOSTART_ARG}")
}

/// The `Run` value as stored, if any.
fn run_value() -> Option<String> {
    let key = Key::open(RUN_KEY, KEY_READ)?;
    let mut buf = [0u8; 4096];
    let (_, len) = key.query(RUN_VALUE, &mut buf)?;
    let units: Vec<u16> = buf[..len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// Executable path of a stored `Run` command line (quoted or not).
fn registered_exe(stored: &str) -> &str {
    let path = stored.trim();
    match path.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => path.split(" --").next().unwrap_or(path),
    }
    .trim()
}

/// Whether a stored `Run` value starts *this* executable but predates the
/// `--autostart` flag. Entries for another copy are never touched.
fn needs_flag_upgrade(stored: &str, exe: &str) -> bool {
    registered_exe(stored).eq_ignore_ascii_case(exe) && !stored.contains(AUTOSTART_ARG)
}

/// Add `--autostart` to an older startup entry of this executable. Only
/// the command line changes: a Task Manager "Disabled" mark stays.
pub fn upgrade_autostart_entry() {
    let (Some(stored), Ok(exe)) = (run_value(), std::env::current_exe()) else {
        return;
    };
    if needs_flag_upgrade(&stored, &exe.to_string_lossy()) {
        let _ = write_run_value();
    }
}

fn approved_says_disabled(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 1 == 1)
}

fn startup_disabled_by_user() -> bool {
    let Some(key) = Key::open(APPROVED_KEY, KEY_READ) else {
        return false;
    };
    let mut buf = [0u8; 16];
    key.query(RUN_VALUE, &mut buf)
        .is_some_and(|(_, len)| approved_says_disabled(&buf[..len]))
}

/// Forget the Task Manager mark (a missing value counts as enabled).
fn clear_startup_approved() {
    if let Some(key) = Key::open(APPROVED_KEY, KEY_WRITE) {
        let _ = key.delete(RUN_VALUE);
    }
}

/// Whether Windows will start the program at sign-in through the `Run`
/// value: present and not disabled in Task Manager.
pub fn autostart_enabled() -> bool {
    run_value().is_some_and(|v| !v.trim().is_empty()) && !startup_disabled_by_user()
}

/// Apply the startup toggle of the settings form. Writes only when the
/// state differs, or when the entry starts an executable that no longer
/// exists — so saving from another copy (a test build) never takes over a
/// working entry of the installed one.
pub fn sync_autostart(enable: bool) -> bool {
    let target_exists = || run_value().is_some_and(|v| Path::new(registered_exe(&v)).is_file());
    if enable == autostart_enabled() && (!enable || target_exists()) {
        return true;
    }
    set_autostart(enable)
}

fn set_autostart(enable: bool) -> bool {
    if !enable {
        let Some(key) = Key::open(RUN_KEY, KEY_WRITE) else {
            return false;
        };
        // A stale "Disabled" mark would silently override a later enable
        // made by another tool or an older version.
        clear_startup_approved();
        return key.delete(RUN_VALUE);
    }
    // An explicit enable also lifts a Task Manager "Disabled" mark, or the
    // entry would be written but never run.
    let ok = write_run_value();
    if ok {
        clear_startup_approved();
    }
    ok
}

/// Write the `Run` value for this executable.
fn write_run_value() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(key) = Key::create(RUN_KEY) else {
        return false;
    };
    // Quote so install locations containing spaces keep launching
    // (ISSUE-002). The NUL terminator is included in the byte count.
    let bytes: Vec<u8> = wide(&quote_run_path(&exe.to_string_lossy()))
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    key.set(RUN_VALUE, REG_SZ, &bytes)
}

// ---- Elevated sign-in task -------------------------------------------------

/// Why the elevated sign-in task could not be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskError {
    /// This process was elevated with another administrator's credentials:
    /// a task registered now would start for that account, not for the
    /// signed-in user.
    OtherAccount,
    Failed,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Task definition: at sign-in of `user` (a SID string), run `exe` with
/// highest privileges, also on battery, with no time limit (the scheduler's
/// defaults would skip laptops on battery and stop the program after 72
/// hours).
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

/// SID string of the user a process runs as.
fn process_user_sid(process: HANDLE) -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        // TOKEN_USER plus the SID it points into; u64 keeps it aligned.
        let mut buf = [0u64; 64];
        let mut len = 0u32;
        let got = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            std::mem::size_of_val(&buf) as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        got.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid
    }
}

/// SID of the signed-in user: the owner of the desktop shell.
fn shell_user_sid() -> Option<String> {
    unsafe {
        let shell = GetShellWindow();
        if shell.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(shell, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let sid = process_user_sid(process);
        let _ = CloseHandle(process);
        sid
    }
}

/// Run `f` on the root task folder, with COM set up for the call.
fn with_root_folder<R>(
    f: impl FnOnce(&ITaskFolder) -> windows::core::Result<R>,
) -> windows::core::Result<R> {
    unsafe {
        // An STA thread (RPC_E_CHANGED_MODE) can use the scheduler as is.
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| {
            let service: ITaskService = CoCreateInstance(
                &TaskScheduler,
                None::<&windows::core::IUnknown>,
                CLSCTX_INPROC_SERVER,
            )?;
            let none = VARIANT::default();
            service.Connect(&none, &none, &none, &none)?;
            f(&service.GetFolder(&BSTR::from("\\"))?)
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// Executable started by the elevated sign-in task, when the task exists
/// and is enabled (empty when its action cannot be read). Talks to the
/// Task Scheduler service; call off the UI thread.
fn elevated_task() -> Option<String> {
    with_root_folder(|folder| unsafe {
        let task = folder.GetTask(&BSTR::from(TASK_NAME))?;
        if !task.Enabled()?.as_bool() {
            return Ok(None);
        }
        let exe = task
            .Definition()
            .and_then(|d| d.Actions())
            .and_then(|a| a.get_Item(1))
            .and_then(|a| a.cast::<IExecAction>())
            .and_then(|a| {
                let mut path = BSTR::new();
                a.Path(&mut path).map(|_| path.to_string())
            })
            .unwrap_or_default();
        Ok(Some(exe))
    })
    .ok()
    .flatten()
}

fn register_task() -> Result<(), TaskError> {
    let exe = std::env::current_exe().map_err(|_| TaskError::Failed)?;
    let me = unsafe { process_user_sid(GetCurrentProcess()) }.ok_or(TaskError::Failed)?;
    if shell_user_sid().is_some_and(|shell| !shell.eq_ignore_ascii_case(&me)) {
        return Err(TaskError::OtherAccount);
    }
    let xml = task_xml(&exe.to_string_lossy(), &me);
    // Passed in memory: no definition file another process could swap.
    with_root_folder(|folder| unsafe {
        let none = VARIANT::default();
        folder.RegisterTask(
            &BSTR::from(TASK_NAME),
            &BSTR::from(xml.as_str()),
            TASK_CREATE_OR_UPDATE.0,
            &none,
            &none,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &none,
        )
    })
    .map(|_| ())
    .map_err(|_| TaskError::Failed)
}

fn delete_task() -> Result<(), TaskError> {
    with_root_folder(|folder| unsafe {
        match folder.DeleteTask(&BSTR::from(TASK_NAME), 0) {
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            other => other,
        }
    })
    .map_err(|_| TaskError::Failed)
}

/// Create or remove the elevated sign-in task (needs an elevated process;
/// call off the UI thread). The marker follows the result.
pub fn set_elevated_task(enable: bool) -> Result<(), TaskError> {
    let result = if enable {
        register_task()
    } else {
        delete_task()
    };
    if result.is_ok() {
        set_marker(enable);
    }
    result
}

/// At the start of an elevated copy: whether the sign-in task is active.
/// Also brings the marker in step with it (the task may have been removed
/// or disabled in Task Scheduler), and points a task whose executable no
/// longer exists (the program was moved) at this copy.
pub fn refresh_elevated_task() -> bool {
    let task = elevated_task();
    if let Some(exe) = &task
        && !exe.is_empty()
        && !Path::new(exe).is_file()
    {
        let _ = register_task();
    }
    set_marker(task.is_some());
    task.is_some()
}

/// Whether `path` lies inside one of `roots` (whole path components,
/// case-insensitive, as Windows compares paths).
fn is_under_any(path: &str, roots: &[String]) -> bool {
    let norm = |p: &str| {
        let p = p
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase();
        format!("{}\\", p.trim_end_matches('\\'))
    };
    let path = norm(path);
    roots
        .iter()
        .filter(|r| !r.trim().is_empty())
        .any(|root| path.starts_with(&norm(root)))
}

/// Folders only administrators can change: Program Files (both) and Windows.
fn protected_roots() -> Vec<String> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Windows, KF_FLAG_DEFAULT,
            SHGetKnownFolderPath,
        },
    };
    [
        FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86,
        FOLDERID_Windows,
    ]
    .iter()
    .filter_map(|id| unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    })
    .collect()
}

/// Whether this executable sits where only administrators can replace it.
/// Anywhere else (a portable folder, Scoop, winget), a task that starts it
/// elevated at every sign-in lets any program of this account replace the
/// file and gain administrator rights (2026-10-22 audit ISSUE-003).
pub fn exe_in_protected_location() -> bool {
    std::env::current_exe()
        .is_ok_and(|exe| is_under_any(&exe.to_string_lossy(), &protected_roots()))
}

/// Drop the elevated-start marker (a non-elevated automatic start waited
/// for the elevated one in vain: the task is gone or disabled).
pub fn clear_elevated_start_marker() {
    set_marker(false);
}

/// Set the elevated-start marker (the elevated sign-in task just ran).
pub fn mark_elevated_start() {
    set_marker(true);
}

fn set_marker(on: bool) {
    if on {
        if let Some(key) = Key::create(STATE_KEY) {
            let _ = key.set(MARKER_VALUE, REG_DWORD, &1u32.to_le_bytes());
        }
    } else {
        let path = wide(STATE_KEY);
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr())) };
    }
}

/// Whether an elevated sign-in start is registered, so a non-elevated
/// automatic start should give it a chance first. A plain registry read.
pub fn elevated_start_expected() -> bool {
    let Some(key) = Key::open(STATE_KEY, KEY_READ) else {
        return false;
    };
    let mut buf = [0u8; 4];
    key.query(MARKER_VALUE, &mut buf)
        .is_some_and(|(kind, len)| kind == REG_DWORD && len == 4 && buf != [0; 4])
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
    fn registered_exe_handles_quoted_and_bare_entries() {
        let exe = r"C:\Program Files\KCFix\kcfix.exe";
        assert_eq!(registered_exe(&quote_run_path(exe)), exe);
        assert_eq!(registered_exe(&format!(" \"{exe}\" ")), exe);
        assert_eq!(
            registered_exe(r"C:\Tools\kcfix.exe --autostart"),
            r"C:\Tools\kcfix.exe"
        );
        assert_eq!(registered_exe(r"C:\Tools\kcfix.exe"), r"C:\Tools\kcfix.exe");
    }

    #[test]
    fn task_manager_disabled_mark_is_the_odd_first_byte() {
        assert!(!approved_says_disabled(&[0x02, 0, 0, 0]));
        assert!(!approved_says_disabled(&[0x06, 0, 0, 0]));
        assert!(approved_says_disabled(&[0x03, 0, 0, 0, 1, 2, 3, 4]));
        assert!(approved_says_disabled(&[0x07]));
        // Missing or empty data: Windows treats the entry as enabled.
        assert!(!approved_says_disabled(&[]));
    }

    #[test]
    fn protected_location_means_inside_a_protected_root() {
        let roots = [
            r"C:\Program Files".to_string(),
            r"C:\Program Files (x86)".to_string(),
            String::new(),
        ];
        assert!(is_under_any(r"C:\Program Files\KCFix\kcfix.exe", &roots));
        assert!(is_under_any(r"c:\program files (x86)\k\k.exe", &roots));
        assert!(is_under_any(r"\\?\C:\Program Files\K\k.exe", &roots));
        // A sibling whose name merely starts the same is not inside.
        assert!(!is_under_any(r"C:\Program Files Evil\k.exe", &roots));
        assert!(!is_under_any(
            r"C:\Users\kim\scoop\apps\keyboard-chattering-fix\current\k.exe",
            &roots
        ));
        // An empty root (lookup failed) never matches everything.
        assert!(!is_under_any(r"D:\Tools\k.exe", &[String::new()]));
    }

    #[test]
    fn known_protected_roots_resolve() {
        let roots = protected_roots();
        assert!(
            roots
                .iter()
                .any(|r| r.to_lowercase().ends_with(r"\windows")),
            "{roots:?}"
        );
        // The test binary lives in the repository, not under Program Files.
        assert!(!exe_in_protected_location());
    }

    #[test]
    fn our_own_user_sid_is_readable() {
        let sid = unsafe { process_user_sid(GetCurrentProcess()) }.expect("own token");
        assert!(sid.starts_with("S-1-"), "{sid}");
    }

    #[test]
    fn task_definition_runs_elevated_on_battery_without_time_limit() {
        let xml = task_xml(r"C:\Tools & Co\kcfix.exe", "S-1-5-21-1-2-3-1001");
        assert!(xml.contains("<Command>C:\\Tools &amp; Co\\kcfix.exe</Command>"));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
        // Both the trigger and the principal name the user: a logon trigger
        // without one would start the program for every account.
        assert_eq!(
            xml.matches("<UserId>S-1-5-21-1-2-3-1001</UserId>").count(),
            2
        );
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
