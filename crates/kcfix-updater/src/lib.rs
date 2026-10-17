use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    #[error("현재 실행 파일이 존재하지 않습니다: {0}")]
    CurrentMissing(PathBuf),
    #[error("새 업데이트 파일이 존재하지 않습니다: {0}")]
    ReplacementMissing(PathBuf),
    #[error("새 업데이트 파일 크기가 0입니다: {0}")]
    ReplacementEmpty(PathBuf),
    #[error("상위 프로세스({0}) 대기 시간 초과")]
    WaitTimeout(u32),
    #[error("백업 생성 실패: {0}")]
    BackupFailed(String),
    #[error("실행 파일 교체 실패: {0}")]
    ReplaceFailed(String),
    #[error("새 버전 실행 실패: {0}")]
    RelaunchFailed(String),
    #[error("롤백 실패: {0}")]
    RollbackFailed(String),
    #[error("새 업데이트 파일 해시가 일치하지 않습니다: {0}")]
    ReplacementHashMismatch(String),
}

/// Language for user-facing helper text. The app passes it in the
/// `KCFIX_LANG` environment variable ("ko" / "en"), which older apps simply
/// do not set; Korean stays the default for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperLang {
    Ko,
    En,
}

impl HelperLang {
    /// Environment variable the app sets when launching the helper.
    pub const ENV: &'static str = "KCFIX_LANG";

    /// `None` is a pre-`KCFIX_LANG` app, which only ever spoke Korean. Any
    /// code the helper has no text for (a language added to the app later)
    /// gets English rather than Korean.
    pub fn from_code(code: Option<&str>) -> Self {
        match code.map(|c| c.trim().to_ascii_lowercase()) {
            None => Self::Ko,
            Some(c) if c == "ko" => Self::Ko,
            Some(_) => Self::En,
        }
    }

    pub fn from_env() -> Self {
        Self::from_code(std::env::var(Self::ENV).ok().as_deref())
    }

    pub fn failure_title(self) -> &'static str {
        match self {
            Self::Ko => "업데이트 실패",
            Self::En => "Update failed",
        }
    }

    pub fn relaunched_note(self) -> &'static str {
        match self {
            Self::Ko => "이전 버전으로 다시 실행했습니다.",
            Self::En => "The previous version has been restarted.",
        }
    }
}

impl HelperError {
    /// User-facing text in `lang` (the `Display` form stays Korean for logs
    /// and compatibility).
    pub fn message(&self, lang: HelperLang) -> String {
        if lang == HelperLang::Ko {
            return self.to_string();
        }
        match self {
            Self::CurrentMissing(p) => format!("The program file does not exist: {}", p.display()),
            Self::ReplacementMissing(p) => {
                format!("The downloaded update does not exist: {}", p.display())
            }
            Self::ReplacementEmpty(p) => format!("The downloaded update is empty: {}", p.display()),
            Self::WaitTimeout(pid) => {
                format!("Timed out waiting for the running program (pid {pid}) to exit")
            }
            Self::BackupFailed(e) => format!("Could not back up the current version: {e}"),
            Self::ReplaceFailed(e) => format!("Could not replace the program file: {e}"),
            Self::RelaunchFailed(e) => format!("The new version did not start: {e}"),
            Self::RollbackFailed(e) => format!("Restoring the previous version failed: {e}"),
            Self::ReplacementHashMismatch(e) => {
                format!("The downloaded update failed its integrity check: {e}")
            }
        }
    }
}

/// Options for the swap. `expected_sha256` re-verifies the staged artifact just
/// before it replaces the running EXE: the app hashed it at download time, but
/// the helper is a separate process that starts later, so nothing had checked
/// the bytes actually being installed.
#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    pub relaunch: bool,
    pub relaunch_args: Vec<String>,
    pub expected_sha256: Option<String>,
    /// None preserves compatibility for library callers without the app protocol.
    pub readiness_timeout: Option<Duration>,
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), HelperError> {
    if pid == 0 {
        return Ok(());
    }

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };

        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        let Ok(handle) = handle else {
            // Process might already have exited
            return Ok(());
        };
        if handle.is_invalid() {
            return Ok(());
        }

        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait_res = unsafe { WaitForSingleObject(handle, timeout_ms) };
        let _ = unsafe { CloseHandle(handle) };

        if wait_res == WAIT_OBJECT_0 {
            Ok(())
        } else if wait_res == WAIT_TIMEOUT {
            Err(HelperError::WaitTimeout(pid))
        } else {
            // Other wait return, treat as terminated
            Ok(())
        }
    }

    #[cfg(not(windows))]
    {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(50));
            // Non-windows test stub
            break;
        }
        Ok(())
    }
}

/// Whether the helper should start the previous version after a failed
/// update. The app has already exited by the time the helper runs, so without
/// this a failed swap (for example no write access to the install folder) left
/// the user with no protection running. A wait timeout is the exception: the
/// app is still running, so starting another copy would be wrong.
pub fn should_relaunch_previous(err: &HelperError) -> bool {
    !matches!(err, HelperError::WaitTimeout(_))
}

/// Start `current` (the previous version after a failed or rolled-back
/// update). Returns whether the process was spawned.
pub fn relaunch_previous(current: &Path, args: &[String]) -> bool {
    if !current.is_file() {
        return false;
    }
    match Command::new(current).args(args).spawn() {
        Ok(_) => {
            info!(current = %current.display(), "relaunched the previous version after a failed update");
            true
        }
        Err(err) => {
            error!(%err, "could not relaunch the previous version");
            false
        }
    }
}

/// Restore `backup` over `current` after a failed swap. The failed `current`
/// is removed first so the rename lands on a free path (a partial copy may
/// otherwise block the rollback). A failed rollback leaves the user with no
/// executable, so it must never be silent (ISSUE-001).
fn rollback_swap(current: &Path, backup: &Path) -> Result<(), String> {
    // Retry loop (ISSUE-001): freshly swapped executables are routinely
    // held briefly by antivirus/temp-dir scanners, so a single rename
    // attempt can fail with a transient Access Denied right after the
    // child was reaped. Retries are bounded and only run on this failure
    // path, never on success.
    const ROLLBACK_ATTEMPTS: u32 = 4;
    const ROLLBACK_RETRY_WAIT: Duration = Duration::from_millis(150);
    if let Err(err) = std::fs::remove_file(current) {
        warn!(current = %current.display(), %err, "rollback could not remove failed replacement");
    }
    let mut attempt = 0;
    loop {
        match std::fs::rename(backup, current) {
            Ok(()) => return Ok(()),
            Err(err) if attempt + 1 < ROLLBACK_ATTEMPTS => {
                attempt += 1;
                warn!(%err, attempt, "rollback rename blocked; retrying");
                std::thread::sleep(ROLLBACK_RETRY_WAIT);
            }
            Err(err) => {
                error!(%err, "catastrophic: rollback failed");
                return Err(err.to_string());
            }
        }
    }
}

/// Remove a staged replacement that will not be installed.
pub fn discard_replacement(replacement: &Path) {
    if replacement.is_file() {
        let _ = std::fs::remove_file(replacement);
    }
}

pub fn update_executable(
    current: &Path,
    replacement: &Path,
    pid: u32,
    timeout: Duration,
    relaunch: bool,
) -> Result<(), HelperError> {
    update_executable_with(
        current,
        replacement,
        pid,
        timeout,
        &UpdateOptions {
            relaunch,
            ..UpdateOptions::default()
        },
    )
}

pub fn update_executable_with(
    current: &Path,
    replacement: &Path,
    pid: u32,
    timeout: Duration,
    options: &UpdateOptions,
) -> Result<(), HelperError> {
    let relaunch = options.relaunch;
    info!(
        current = %current.display(),
        replacement = %replacement.display(),
        pid,
        "starting self-update replacement"
    );

    // 1. Wait for target process termination
    wait_for_process_exit(pid, timeout)?;

    // 2. Validate paths
    if !current.exists() {
        return Err(HelperError::CurrentMissing(current.to_path_buf()));
    }
    if !replacement.exists() {
        return Err(HelperError::ReplacementMissing(replacement.to_path_buf()));
    }
    let meta = std::fs::metadata(replacement)
        .map_err(|_e| HelperError::ReplacementMissing(replacement.to_path_buf()))?;
    if meta.len() == 0 {
        return Err(HelperError::ReplacementEmpty(replacement.to_path_buf()));
    }
    if let Some(expected) = options.expected_sha256.as_deref() {
        let actual = sha256_file(replacement)
            .map_err(|err| HelperError::ReplacementHashMismatch(err.to_string()))?;
        if !actual.eq_ignore_ascii_case(expected) {
            error!(%actual, %expected, "staged update failed re-verification");
            let _ = std::fs::remove_file(replacement);
            return Err(HelperError::ReplacementHashMismatch(format!(
                "expected {expected}, got {actual}"
            )));
        }
        info!("staged update re-verified before replacement");
    }

    // 3. Prepare backup path
    let backup = current.with_extension("exe.old");
    if backup.exists() {
        if let Err(err) = std::fs::remove_file(&backup) {
            warn!(%err, "could not remove stale update backup");
        }
    }

    // 4. Rename current -> current.exe.old
    if let Err(err) = std::fs::rename(current, &backup) {
        error!(%err, "failed to rename current executable to backup");
        return Err(HelperError::BackupFailed(err.to_string()));
    }

    // 5. Move replacement -> current
    let replace_res = if let Err(err) = std::fs::rename(replacement, current) {
        // Fallback to copy and remove in case of cross-device temp folder
        match std::fs::copy(replacement, current) {
            Ok(_) => {
                let _ = std::fs::remove_file(replacement);
                Ok(())
            }
            Err(copy_err) => {
                error!(rename_err = %err, %copy_err, "failed to move replacement into place");
                Err(HelperError::ReplaceFailed(format!("{err}; {copy_err}")))
            }
        }
    } else {
        Ok(())
    };

    if let Err(replace_err) = replace_res {
        // Rollback backup -> current
        warn!("attempting rollback after replacement failure");
        if let Err(rb_err) = rollback_swap(current, &backup) {
            // Failure is logged inside rollback_swap; keep the context chain.
            return Err(HelperError::RollbackFailed(format!(
                "{replace_err}; rollback error: {rb_err}"
            )));
        }
        return Err(replace_err);
    }

    // 6. Relaunch new version if requested
    if relaunch {
        info!(args = ?options.relaunch_args, "relaunching updated application");
        // Carry the original launch flags across the update so a
        // minimized/tray launch keeps behaving like one.
        let mut cmd = Command::new(current);
        cmd.args(&options.relaunch_args);
        if let Err(err) = launch_verified(&mut cmd, options.readiness_timeout) {
            error!(%err, "new app did not become ready; rolling back");
            // launch_verified reaps our child before returning an error, so
            // Windows no longer has its image locked during rollback.
            // Removal of the failed image happens inside rollback_swap.
            if let Err(rb) = rollback_swap(current, &backup) {
                return Err(HelperError::RollbackFailed(format!("{err}; {rb}")));
            }
            return Err(HelperError::RelaunchFailed(err));
        }
    }

    // 7. Cleanup backup if possible
    if let Err(err) = std::fs::remove_file(&backup) {
        warn!(%err, "could not remove update backup; the next run removes stale backups");
    }
    info!("self-update completed successfully");
    Ok(())
}

/// The helper owns both this directory and the child it may stop. A readiness
/// file from another/previous process cannot acknowledge this launch.
fn launch_verified(cmd: &mut Command, timeout: Option<Duration>) -> Result<(), String> {
    let Some(timeout) = timeout else {
        return cmd.spawn().map(|_| ()).map_err(|e| e.to_string());
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kcfix-ready-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("ready");
    let result = (|| {
        let mut child = cmd
            .env("KCFIX_UPDATE_READY_FILE", &path)
            .spawn()
            .map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + timeout;
        let mut acknowledged: Option<std::time::Instant> = None;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Err(format!("app exited before readiness: {status}")),
                Ok(None) => {}
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err.to_string());
                }
            }
            if std::fs::read_to_string(&path)
                .ok()
                .is_some_and(|s| s == child.id().to_string())
            {
                let at = acknowledged.get_or_insert_with(std::time::Instant::now);
                if at.elapsed() >= Duration::from_millis(250) {
                    return Ok(());
                }
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("app readiness timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(tag: &str) -> PathBuf {
        let n = SCRATCH_SEQ.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("kcfix_libtest_{}_{}_{tag}", std::process::id(), n))
    }

    #[test]
    fn rollback_swap_restores_backup_over_failed_replacement() {
        let dir = scratch_dir("rollback");
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("app.exe");
        let backup = dir.join("app.exe.old");
        std::fs::write(&current, b"BROKEN").unwrap();
        std::fs::write(&backup, b"PREVIOUS").unwrap();
        assert!(rollback_swap(&current, &backup).is_ok());
        assert_eq!(std::fs::read(&current).unwrap(), b"PREVIOUS");
        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn helper_lang_defaults_to_korean_and_localizes_english() {
        assert_eq!(HelperLang::from_code(None), HelperLang::Ko);
        assert_eq!(HelperLang::from_code(Some("ko")), HelperLang::Ko);
        assert_eq!(HelperLang::from_code(Some(" EN ")), HelperLang::En);
        // A language the app gains later still gets readable English.
        assert_eq!(HelperLang::from_code(Some("ja")), HelperLang::En);
        let err = HelperError::WaitTimeout(7);
        assert_eq!(err.message(HelperLang::Ko), err.to_string());
        let en = err.message(HelperLang::En);
        assert!(en.contains("pid 7") && en.is_ascii(), "{en}");
        assert_ne!(
            HelperLang::Ko.failure_title(),
            HelperLang::En.failure_title()
        );
    }

    #[test]
    fn rollback_swap_reports_missing_backup() {
        let dir = scratch_dir("rollback-missing");
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("app.exe");
        let backup = dir.join("missing.old");
        std::fs::write(&current, b"BROKEN").unwrap();
        let err = rollback_swap(&current, &backup).unwrap_err();
        assert!(!err.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
