use super::super::error::{UpdateError, UpdateErrorKind};
use super::super::model::StagedUpdate;

/// Launch flags worth carrying across an update. kcfix currently relaunches
/// without arguments; `--minimized` is preserved for forward compatibility
/// with a future tray-launch flag.
pub fn relaunch_args_from<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .filter(|arg| matches!(arg.as_ref(), "--minimized"))
        .map(|arg| arg.as_ref().to_string())
        .collect()
}

/// Seconds the helper waits for this process to exit. Generous on purpose:
/// the app exits right after launching the helper, but a slow shutdown
/// (settings flush, antivirus scan) must not strand the update (ISSUE-003).
pub const HELPER_EXIT_TIMEOUT_SECS: u64 = 120;

/// Environment variable carrying the UI language to the helper. An
/// environment variable (not a flag) so helpers from older releases, whose
/// command line rejects unknown flags, simply ignore it (ISSUE-006).
pub const HELPER_LANG_ENV: &str = "KCFIX_LANG";

/// Command-line arguments for the helper. Uses only flags every supported
/// helper (`HELPER_MIN_VERSION` and later) understands.
pub fn helper_args(staged: &StagedUpdate, pid: u32) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--pid".into(),
        pid.to_string().into(),
        "--current".into(),
        staged.current_exe.clone().into(),
        "--replacement".into(),
        staged.replacement.clone().into(),
        "--timeout-secs".into(),
        HELPER_EXIT_TIMEOUT_SECS.to_string().into(),
    ];
    if !staged.sha256.is_empty() {
        args.push("--sha256".into());
        args.push(staged.sha256.clone().into());
    }
    // `=` form: relaunch flags start with `--`, which clap would otherwise
    // read as an unknown flag and abort the helper (contract-tested in
    // `crates/kcfix-updater/src/main.rs`).
    for arg in &staged.relaunch_args {
        args.push(format!("--relaunch-arg={arg}").into());
    }
    args
}

pub fn launch_helper(
    staged: &StagedUpdate,
    lang: crate::i18n::Language,
) -> Result<(), UpdateError> {
    let mut cmd = std::process::Command::new(&staged.helper);
    cmd.args(helper_args(staged, std::process::id()))
        .env(HELPER_LANG_ENV, lang.code());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| UpdateError::with_detail(UpdateErrorKind::HelperLaunch, e))?;
    Ok(())
}
