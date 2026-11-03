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

use super::consts::{AUTOSTART_ARG, TASK_NAME};
use super::marker::set_marker;
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
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
    core::{BSTR, Interface, PWSTR},
};

/// Why the elevated sign-in task could not be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskError {
    /// This process was elevated with another administrator's credentials:
    /// a task registered now would start for that account, not for the
    /// signed-in user.
    OtherAccount,
    Failed,
}

pub(super) fn xml_escape(s: &str) -> String {
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
pub(super) fn process_user_sid(process: HANDLE) -> Option<String> {
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
pub(super) fn shell_user_sid() -> Option<String> {
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
pub(super) fn with_root_folder<R>(
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
pub(super) fn elevated_task() -> Option<String> {
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

pub(super) fn register_task() -> Result<(), TaskError> {
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

pub(super) fn delete_task() -> Result<(), TaskError> {
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
