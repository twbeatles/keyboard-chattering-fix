use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::error::{UpdateError, UpdateErrorKind};
use super::http::download_and_verify;
use super::model::{STAGING_SEQ, StagedUpdate, UPDATE_IN_PROGRESS, UpdateManifest};

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

/// Whether an update is running: the menu bar and tray menu grey out their
/// "Check for updates" item meanwhile (the flow itself guards with
/// [`try_begin_update`]).
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

/// `ProductName` stamped into every helper build (`crates/kcfix-updater/build.rs`).
const HELPER_PRODUCT_NAME: &str = "Keyboard Chatter Fix Updater";

/// Oldest helper whose command line this app still speaks. The helper is
/// never replaced by an in-app update (only the main executable is), so the
/// helper beside the app is usually OLDER than the app after the first
/// update. Its CLI (`--pid --current --replacement --sha256 --relaunch-arg
/// --timeout-secs`) is therefore append-only; raise this floor only when a
/// change breaks that contract (ISSUE-001).
const HELPER_MIN_VERSION: &str = "0.2.0";

/// Identity read from a binary's version resource.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryStamp {
    /// Fixed `FILEVERSION` quad, e.g. "0.2.0.0".
    version: String,
    /// `StringFileInfo\...\ProductName`, when present.
    product_name: Option<String>,
}

/// Version stamp embedded in a binary, if any.
fn file_stamp(path: &Path) -> Option<BinaryStamp> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
        },
        core::{HSTRING, PCWSTR},
    };
    unsafe {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut blob = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, blob.as_mut_ptr().cast()).is_err()
        {
            return None;
        }
        let mut info_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut info_len = 0u32;
        if VerQueryValueW(
            blob.as_ptr().cast(),
            windows::core::w!("\\"),
            &mut info_ptr,
            &mut info_len,
        ) == windows::Win32::Foundation::FALSE
        {
            return None;
        }
        if info_ptr.is_null() || info_len as usize != std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let info = &*(info_ptr as *const VS_FIXEDFILEINFO);
        let (ms, ls) = (info.dwFileVersionMS, info.dwFileVersionLS);
        let version = format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF);

        // ProductName lives under the first translation's string table.
        let mut product_name = None;
        let mut tr_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut tr_len = 0u32;
        if VerQueryValueW(
            blob.as_ptr().cast(),
            windows::core::w!("\\VarFileInfo\\Translation"),
            &mut tr_ptr,
            &mut tr_len,
        ) != windows::Win32::Foundation::FALSE
            && !tr_ptr.is_null()
            && tr_len >= 4
        {
            let pair = std::slice::from_raw_parts(tr_ptr as *const u16, 2);
            let key = HSTRING::from(format!(
                "\\StringFileInfo\\{:04x}{:04x}\\ProductName",
                pair[0], pair[1]
            ));
            let mut name_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut name_len = 0u32;
            if VerQueryValueW(blob.as_ptr().cast(), &key, &mut name_ptr, &mut name_len)
                != windows::Win32::Foundation::FALSE
                && !name_ptr.is_null()
                && name_len > 0
            {
                let units = std::slice::from_raw_parts(name_ptr as *const u16, name_len as usize);
                let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
                product_name = Some(String::from_utf16_lossy(&units[..end]));
            }
        }
        Some(BinaryStamp {
            version,
            product_name,
        })
    }
}

/// Whether a helper candidate may be staged for an app at `app_version`.
///
/// - No version resource: predates stamping, accepted as legacy.
/// - Stamped: must identify as the helper (`ProductName`) and lie within
///   `[HELPER_MIN_VERSION, app_version]`. Requiring an exact match (the old
///   rule) rejected the helper left over from the previous release after the
///   first in-app update, breaking every later update (ISSUE-001).
fn helper_compatible(stamp: Option<&BinaryStamp>, app_version: &str) -> bool {
    let Some(stamp) = stamp else {
        tracing::debug!("helper candidate has no version stamp; accepting as legacy");
        return true;
    };
    if stamp.product_name.as_deref() != Some(HELPER_PRODUCT_NAME) {
        tracing::warn!(name = ?stamp.product_name, "rejecting helper candidate: not the kcfix updater");
        return false;
    }
    let too_new = super::version::is_newer(&stamp.version, app_version).unwrap_or(true);
    let too_old = super::version::is_newer(HELPER_MIN_VERSION, &stamp.version).unwrap_or(true);
    if too_new || too_old {
        tracing::warn!(stamp = %stamp.version, app_version, "rejecting helper candidate outside the supported version range");
        return false;
    }
    true
}

pub fn resolve_helper(current_exe: &Path) -> Result<PathBuf, UpdateError> {
    let current_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    // The copy beside the app is authoritative: a present-but-unusable file
    // fails loudly instead of silently falling through to a dev-tree copy.
    let beside = current_dir.join(HELPER_FILENAME);
    if beside.exists() {
        if !helper_looks_valid(&beside) {
            return Err(UpdateError::new(UpdateErrorKind::HelperCorrupt));
        }
        if !helper_compatible(file_stamp(&beside).as_ref(), crate::config::VERSION) {
            return Err(UpdateError::new(UpdateErrorKind::HelperVersionMismatch));
        }
        return Ok(beside);
    }
    // Dev-tree fallbacks (best effort, same validity bar).
    for cand in [
        current_dir.join("target/release").join(HELPER_FILENAME),
        current_dir.join("target/debug").join(HELPER_FILENAME),
        current_dir.join("../target/release").join(HELPER_FILENAME),
        current_dir.join("../target/debug").join(HELPER_FILENAME),
    ] {
        if helper_looks_valid(&cand)
            && helper_compatible(file_stamp(&cand).as_ref(), crate::config::VERSION)
        {
            return Ok(cand);
        }
    }
    Err(UpdateError::new(UpdateErrorKind::HelperMissing))
}

/// `--check-helper`: resolve the helper the next update would use and
/// describe it ("<path> (<version stamp>|unstamped)"). Lets a release smoke
/// check and users confirm the install folder can still update itself.
pub fn describe_helper(current_exe: &Path) -> Result<String, UpdateError> {
    let helper = resolve_helper(current_exe)?;
    let stamp = file_stamp(&helper)
        .map(|s| s.version)
        .unwrap_or_else(|| "unstamped".to_string());
    Ok(format!("{} ({stamp})", helper.display()))
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
fn is_staging_file_name(name: &str) -> bool {
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

fn cleanup_staging_dir(dir: &Path, max_age: Duration) {
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
        assert!(
            left.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("0.2.0")
        );
    }

    #[test]
    fn cleanup_keeps_fresh_staging_and_drops_only_old() {
        let dir = std::env::temp_dir().join(format!(
            "kcfix_staging_age_{}_{}",
            std::process::id(),
            STAGING_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = dir.join("keyboardchatteringfix_update_0.9.9_1_2_3.exe");
        let other = dir.join("unrelated.exe");
        std::fs::write(&fresh, b"fake").unwrap();
        std::fs::write(&other, b"fake").unwrap();
        // An hour's grace: a fresh in-flight file survives (ISSUE-004).
        cleanup_staging_dir(&dir, Duration::from_secs(3600));
        assert!(fresh.exists());
        assert!(other.exists());
        // Zero grace: the leftover is recognized and removed, others kept.
        cleanup_staging_dir(&dir, Duration::ZERO);
        assert!(!fresh.exists());
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn file_stamp_reads_version_and_product_name() {
        // kernel32.dll always carries version info: a numeric quad plus a
        // product name that is NOT the kcfix helper.
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let stamp = file_stamp(&system.join("System32").join("kernel32.dll"))
            .expect("kernel32.dll should carry a version stamp");
        let parts: Vec<&str> = stamp.version.split('.').collect();
        assert_eq!(parts.len(), 4);
        assert!(
            parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        );
        let name = stamp.product_name.as_deref().expect("product name");
        assert!(!name.is_empty());
        // A foreign stamped binary is never accepted as the helper.
        assert!(!helper_compatible(Some(&stamp), crate::config::VERSION));
    }

    #[test]
    fn file_stamp_returns_none_without_stamp() {
        let dir = std::env::temp_dir().join(format!("kcfix_version_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.bin");
        std::fs::write(&junk, b"not-an-executable").unwrap();
        assert_eq!(file_stamp(&junk), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn helper_stamp(version: &str) -> BinaryStamp {
        BinaryStamp {
            version: version.to_string(),
            product_name: Some(HELPER_PRODUCT_NAME.to_string()),
        }
    }

    #[test]
    fn helper_compatible_accepts_older_supported_helpers() {
        // Unstamped binaries predate stamping: accepted for upgrade compatibility.
        assert!(helper_compatible(None, "0.3.0"));
        // Same version, plain or quad.
        assert!(helper_compatible(Some(&helper_stamp("0.3.0.0")), "0.3.0"));
        // ISSUE-001: the helper left over from the previous release must keep
        // working after the app itself was updated — repeatedly.
        assert!(helper_compatible(Some(&helper_stamp("0.2.0.0")), "0.3.0"));
        assert!(helper_compatible(Some(&helper_stamp("0.2.0.0")), "1.4.2"));
    }

    #[test]
    fn helper_compatible_rejects_out_of_range_or_foreign() {
        // Older than the CLI contract floor.
        assert!(!helper_compatible(Some(&helper_stamp("0.1.9.0")), "0.3.0"));
        // Newer than the app: a helper from the future is not trusted.
        assert!(!helper_compatible(Some(&helper_stamp("0.4.0.0")), "0.3.0"));
        // Right version, wrong product.
        let foreign = BinaryStamp {
            version: "0.3.0.0".to_string(),
            product_name: Some("Something Else".to_string()),
        };
        assert!(!helper_compatible(Some(&foreign), "0.3.0"));
        let nameless = BinaryStamp {
            version: "0.3.0.0".to_string(),
            product_name: None,
        };
        assert!(!helper_compatible(Some(&nameless), "0.3.0"));
    }

    #[test]
    fn helper_args_pass_exit_timeout_and_hash() {
        let staged = StagedUpdate {
            helper: PathBuf::from(r"C:\t\h.exe"),
            current_exe: PathBuf::from(r"C:\app\a.exe"),
            replacement: PathBuf::from(r"C:\t\r.exe"),
            sha256: "ab".repeat(32),
            relaunch_args: vec!["--minimized".to_string()],
        };
        let args: Vec<String> = helper_args(&staged, 42)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let after = |flag: &str| {
            let i = args.iter().position(|a| a == flag).expect(flag);
            args[i + 1].clone()
        };
        assert_eq!(after("--pid"), "42");
        assert_eq!(after("--current"), r"C:\app\a.exe");
        assert_eq!(after("--replacement"), r"C:\t\r.exe");
        // ISSUE-003: never fall back to the helper's 30 s default.
        assert_eq!(
            after("--timeout-secs"),
            HELPER_EXIT_TIMEOUT_SECS.to_string()
        );
        const { assert!(HELPER_EXIT_TIMEOUT_SECS >= 120) };
        assert_eq!(after("--sha256"), "ab".repeat(32));
        assert!(args.iter().any(|a| a == "--relaunch-arg=--minimized"));
        // Only flags HELPER_MIN_VERSION understands: no language flag.
        assert!(!args.iter().any(|a| a == "--lang"));
    }

    #[test]
    fn staging_leftovers_are_recognized_by_name_only() {
        assert!(is_staging_file_name(
            "kcfix-updater_helper_1234_1700000000_0.exe"
        ));
        assert!(is_staging_file_name(
            "KeyboardChatteringFix_update_0.3.0_1_2_3.exe"
        ));
        assert!(!is_staging_file_name("kcfix-updater.exe"));
        assert!(!is_staging_file_name("KeyboardChatteringFix.exe"));
        assert!(!is_staging_file_name("kcfix-updater_helper_1.txt"));
        // The real staging generator matches the recognizer.
        let made = unique_staging_path("kcfix-updater", "helper");
        assert!(is_staging_file_name(
            &made.file_name().unwrap().to_string_lossy()
        ));
        let made = unique_staging_path("KeyboardChatteringFix_update", "0.3.0");
        assert!(is_staging_file_name(
            &made.file_name().unwrap().to_string_lossy()
        ));
    }
}
