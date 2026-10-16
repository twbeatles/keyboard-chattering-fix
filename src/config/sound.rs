//! Sound-path validation, done once outside the keyboard hook.
//!
//! The hook must never touch the filesystem per keystroke (ISSUE-003), so
//! the configured string is resolved to an existing file here — at startup
//! and at settings-save time — and only the validated result is stored.
//! The UTF-16 encoding is precomputed too, so the hook plays the sound with
//! zero allocation on its hot path.

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

/// A sound file validated once: the path is known to exist and its
/// NUL-terminated UTF-16 form is pre-encoded for `PlaySoundW`.
#[derive(Debug)]
pub struct ValidatedSound {
    /// Audit trail of what was validated. Only `wide` is read on the hot
    /// path; the field documents the source file for diagnostics.
    #[allow(dead_code)]
    pub path: PathBuf,
    pub wide: Vec<u16>,
}

/// Validate a configured sound path once, outside the keyboard hook.
/// Returns the validated sound only when the path points at an existing
/// file, so the hook never performs filesystem I/O per blocked keystroke
/// (ISSUE-003).
pub fn resolve_sound_file(raw: Option<&str>) -> Option<ValidatedSound> {
    let raw = raw.filter(|s| !s.trim().is_empty())?;
    let path = PathBuf::from(raw.trim());
    if !path.is_file() {
        return None;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    Some(ValidatedSound { path, wide })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_sound_file_accepts_only_existing_files() {
        assert_eq!(resolve_sound_file(None).map(|s| s.path), None);
        assert_eq!(resolve_sound_file(Some("")).map(|s| s.path), None);
        assert_eq!(resolve_sound_file(Some("   ")).map(|s| s.path), None);
        assert_eq!(
            resolve_sound_file(Some(r"C:\definitely\not\hereibly.wav")).map(|s| s.path),
            None
        );
        let wav = std::env::temp_dir().join(format!("kcfix_sndtest_{}.wav", std::process::id()));
        std::fs::write(&wav, b"RIFF").unwrap();
        let validated = resolve_sound_file(wav.to_str()).expect("existing wav validates");
        assert_eq!(validated.path, wav);
        // The cached encoding must round-trip to the same path plus NUL.
        let decoded = String::from_utf16_lossy(&validated.wide);
        assert_eq!(decoded, format!("{}\0", wav.to_string_lossy()));
        let _ = std::fs::remove_file(&wav);
    }
}
