use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::error::{UpdateError, UpdateErrorKind};
use super::super::http::download_and_verify;
use super::super::model::{STAGING_SEQ, StagedUpdate, UpdateManifest};
use super::helper::resolve_helper;
use super::launch::{launch_helper, relaunch_args_from};

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
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::HelperStage, err))?;
    Ok(dest)
}

pub fn prepare_update(manifest: &UpdateManifest) -> Result<StagedUpdate, UpdateError> {
    let current_exe = std::env::current_exe()
        .map_err(|e| UpdateError::with_detail(UpdateErrorKind::ExePath, e))?;
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

/// Delete staged files when the helper could not be started, so repeated
/// failures do not accumulate executables in %TEMP%.
pub fn discard_staged(staged: &StagedUpdate) {
    if let Err(err) = std::fs::remove_file(&staged.helper) {
        tracing::warn!(%err, "could not discard staged helper");
    }
    if let Err(err) = std::fs::remove_file(&staged.replacement) {
        tracing::warn!(%err, "could not discard staged replacement");
    }
}

/// File names `unique_staging_path` produces for the helper copy and the
/// downloaded replacement.
pub(super) fn is_staging_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".exe")
        && (lower.starts_with("kcfix-updater_helper_")
            || lower.starts_with("keyboardchatteringfix_update_"))
}

/// Staging files younger than this are never deleted: no update step holds
/// one longer than a few minutes (helper wait 120 s + readiness 60 s), so a
/// younger file may belong to an update that is still in flight — e.g. after
/// the user relaunched the app while the helper waited (ISSUE-004).
const STAGING_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Remove staging leftovers in %TEMP%: the helper copy always survives a
/// successful update (a running helper cannot delete itself), and a crash can
/// strand a replacement (G4). Runs once at startup, after the single-instance
/// guard. Only files older than [`STAGING_MAX_AGE`] go: a younger file may
/// belong to an update that is still in flight. A helper that is still
/// running keeps its image locked and is simply skipped.
pub fn cleanup_stale_staging() {
    cleanup_staging_dir(&std::env::temp_dir(), STAGING_MAX_AGE);
}

pub(super) fn cleanup_staging_dir(dir: &Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        if !is_staging_file_name(&name.to_string_lossy()) || !path.is_file() {
            continue;
        }
        // A missing mtime or a fresh file may be an update still in flight:
        // keep it.
        let stale = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age >= max_age);
        if stale && let Err(err) = std::fs::remove_file(&path) {
            tracing::debug!(%err, file = ?name, "staging leftover still in use");
        }
    }
}

// One-shot convenience wrapper (prepare + launch). The tray flow uses the
// two steps separately so staged files can be discarded on launch failure.
#[allow(dead_code)]
pub fn apply_update(
    manifest: &UpdateManifest,
    lang: crate::i18n::Language,
) -> Result<(), UpdateError> {
    let staged = prepare_update(manifest)?;
    launch_helper(&staged, lang)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
