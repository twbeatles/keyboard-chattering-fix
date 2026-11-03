use std::path::{Path, PathBuf};

use super::super::error::{UpdateError, UpdateErrorKind};
use super::super::version::is_newer;

/// Helper binary name. It must sit next to the app executable so the update
/// can proceed without extra installs or elevated privileges.
pub const HELPER_FILENAME: &str = "kcfix-updater.exe";

/// `ProductName` stamped into every helper build (`crates/kcfix-updater/build.rs`).
pub(super) const HELPER_PRODUCT_NAME: &str = "Keyboard Chatter Fix Updater";

/// Oldest helper whose command line this app still speaks. The helper is
/// never replaced by an in-app update (only the main executable is), so the
/// helper beside the app is usually OLDER than the app after the first
/// update. Its CLI (`--pid --current --replacement --sha256 --relaunch-arg
/// --timeout-secs`) is therefore append-only; raise this floor only when a
/// change breaks that contract (ISSUE-001).
const HELPER_MIN_VERSION: &str = "0.2.0";

/// Identity read from a binary's version resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BinaryStamp {
    /// Fixed `FILEVERSION` quad, e.g. "0.2.0.0".
    pub(super) version: String,
    /// `StringFileInfo\...\ProductName`, when present.
    pub(super) product_name: Option<String>,
}

/// Version stamp embedded in a binary, if any.
pub(super) fn file_stamp(path: &Path) -> Option<BinaryStamp> {
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
pub(super) fn helper_compatible(stamp: Option<&BinaryStamp>, app_version: &str) -> bool {
    let Some(stamp) = stamp else {
        tracing::debug!("helper candidate has no version stamp; accepting as legacy");
        return true;
    };
    if stamp.product_name.as_deref() != Some(HELPER_PRODUCT_NAME) {
        tracing::warn!(name = ?stamp.product_name, "rejecting helper candidate: not the kcfix updater");
        return false;
    }
    let too_new = is_newer(&stamp.version, app_version).unwrap_or(true);
    let too_old = is_newer(HELPER_MIN_VERSION, &stamp.version).unwrap_or(true);
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
