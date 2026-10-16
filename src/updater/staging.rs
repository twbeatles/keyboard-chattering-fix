use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::UpdateError;
use super::http::download_and_verify;
use super::model::{StagedUpdate, UpdateManifest, STAGING_SEQ, UPDATE_IN_PROGRESS};

/// Helper binary name. It must sit next to the app executable so the update
/// can proceed without extra installs or elevated privileges.
pub const HELPER_FILENAME: &str = "kcfix-updater.exe";

pub fn try_begin_update() -> bool {
    UPDATE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn end_update() {
    UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
}

// Kept for API parity with the reference updater (used by future automatic
// background checks); the current manual flow guards with `try_begin_update`.
#[allow(dead_code)]
pub fn update_in_progress() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}

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

pub fn resolve_helper(current_exe: &Path) -> Result<PathBuf, UpdateError> {
    let current_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = vec![current_dir.join(HELPER_FILENAME)];
    candidates.extend([
        current_dir.join("target/release").join(HELPER_FILENAME),
        current_dir.join("target/debug").join(HELPER_FILENAME),
        current_dir.join("../target/release").join(HELPER_FILENAME),
        current_dir.join("../target/debug").join(HELPER_FILENAME),
    ]);
    for cand in &candidates {
        if helper_looks_valid(cand) {
            return Ok(cand.clone());
        }
    }
    Err(UpdateError::Message(
        "kcfix-updater.exe 헬퍼 바이너리를 찾을 수 없습니다. 앱과 같은 폴더에 함께 두세요.".into(),
    ))
}

fn helper_looks_valid(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z'
}

pub fn unique_staging_path(prefix: &str, version: &str) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "{prefix}_{version}_{}_{}_{}.exe",
        std::process::id(),
        unix_now(),
        seq
    ))
}

pub fn stage_helper(src: &Path) -> Result<PathBuf, UpdateError> {
    let dest = unique_staging_path("kcfix-updater", "helper");
    std::fs::copy(src, &dest)
        .map_err(|err| UpdateError::Message(format!("업데이트 헬퍼 준비 실패: {err}")))?;
    Ok(dest)
}

pub fn prepare_update(manifest: &UpdateManifest) -> Result<StagedUpdate, UpdateError> {
    let current_exe = std::env::current_exe()
        .map_err(|e| UpdateError::Message(format!("현재 실행 파일 경로 확인 실패: {e}")))?;
    let helper_src = resolve_helper(&current_exe)?;
    let helper = stage_helper(&helper_src)?;
    let replacement = unique_staging_path("KeyboardChatteringFix_update", &manifest.version);
    if let Err(err) = download_and_verify(manifest, &replacement) {
        let _ = std::fs::remove_file(&helper);
        let _ = std::fs::remove_file(&replacement);
        return Err(err);
    }
    Ok(StagedUpdate {
        helper,
        current_exe,
        replacement,
        sha256: manifest.sha256.clone(),
        relaunch_args: relaunch_args_from(std::env::args().skip(1)),
    })
}

pub fn launch_helper(staged: &StagedUpdate) -> Result<(), UpdateError> {
    let pid = std::process::id();
    let mut cmd = std::process::Command::new(&staged.helper);
    cmd.arg("--pid")
        .arg(pid.to_string())
        .arg("--current")
        .arg(&staged.current_exe)
        .arg("--replacement")
        .arg(&staged.replacement);
    if !staged.sha256.is_empty() {
        cmd.arg("--sha256").arg(&staged.sha256);
    }
    for arg in &staged.relaunch_args {
        cmd.arg("--relaunch-arg").arg(arg);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| UpdateError::Message(format!("업데이트 헬퍼 실행 실패: {e}")))?;
    Ok(())
}

/// Delete staged files when the helper could not be started, so repeated
/// failures do not accumulate executables in %TEMP%.
pub fn discard_staged(staged: &StagedUpdate) {
    let _ = std::fs::remove_file(&staged.helper);
    let _ = std::fs::remove_file(&staged.replacement);
}

// One-shot convenience wrapper (prepare + launch). The tray flow uses the
// two steps separately so staged files can be discarded on launch failure.
#[allow(dead_code)]
pub fn apply_update(manifest: &UpdateManifest) -> Result<(), UpdateError> {
    let staged = prepare_update(manifest)?;
    launch_helper(&staged)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the process-global single-flight flag,
    /// since the test runner executes test fns on parallel threads.
    static FLAG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn relaunch_args_keep_launch_flags_only() {
        assert_eq!(
            relaunch_args_from(["--minimized"]),
            vec!["--minimized".to_string()]
        );
        assert!(relaunch_args_from(["--version"]).is_empty());
        assert!(relaunch_args_from(["--check-update"]).is_empty());
        assert!(relaunch_args_from(Vec::<&str>::new()).is_empty());
    }

    #[test]
    fn unique_staging_paths_differ() {
        let left = unique_staging_path("KeyboardChatteringFix_update", "0.2.0");
        let right = unique_staging_path("KeyboardChatteringFix_update", "0.2.0");
        assert_ne!(left, right);
        assert!(left
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("0.2.0"));
    }

    #[test]
    fn resolve_helper_rejects_non_mz_and_accepts_mz() {
        let dir = std::env::temp_dir().join(format!("kcfix_helper_resolve_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake_app = dir.join("app.exe");
        std::fs::write(&fake_app, b"MZ-app").unwrap();
        assert!(resolve_helper(&fake_app).is_err());

        let helper = dir.join(HELPER_FILENAME);
        std::fs::write(&helper, b"MZ-helper-bytes").unwrap();
        let found = resolve_helper(&fake_app).unwrap();
        assert_eq!(found, helper);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_begin_update_is_single_flight() {
        let _lock = FLAG_TEST_LOCK.lock().unwrap();
        end_update();
        assert!(try_begin_update());
        assert!(!try_begin_update());
        end_update();
        assert!(try_begin_update());
        end_update();
    }

    #[test]
    fn try_begin_update_parallel_exactly_one_winner() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let _lock = FLAG_TEST_LOCK.lock().unwrap();
        end_update();
        let winners = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    if try_begin_update() {
                        winners.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });
        assert_eq!(winners.load(Ordering::SeqCst), 1);
        end_update();
    }
}
