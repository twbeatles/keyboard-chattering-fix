use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;

use super::canonical::canonical_payload_python;
use super::error::{UpdateError, UpdateErrorKind};
use super::model::{
    UpdateManifest, MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, RELEASE_DOWNLOAD_PREFIX,
};
use super::version::is_newer;
use crate::config::UPDATE_PUBLIC_KEY_B64;

/// File name of the release artifact. Must match `scripts/build_release.ps1`
/// (`-ExeName`), the release workflow, and `docs/UPDATE.md`.
pub const ARTIFACT_FILENAME: &str = "KeyboardChatteringFix.exe";

pub fn expected_artifact_url(tag: &str) -> String {
    format!("{RELEASE_DOWNLOAD_PREFIX}{tag}/{ARTIFACT_FILENAME}")
}

pub fn is_valid_artifact_url(artifact_url: &str, tag: &str) -> bool {
    if !artifact_url.starts_with("https://") {
        return false;
    }
    artifact_url == expected_artifact_url(tag)
}

pub fn parse_and_verify_manifest(
    document: &[u8],
    current_version: &str,
) -> Result<UpdateManifest, UpdateError> {
    if document.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::new(UpdateErrorKind::ManifestInvalid));
    }
    let parsed: Value = serde_json::from_slice(document)
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let payload = parsed
        .get("payload")
        .cloned()
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let signature_b64 = parsed
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let public = base64::engine::general_purpose::STANDARD
        .decode(UPDATE_PUBLIC_KEY_B64)
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let key_bytes: [u8; 32] = public
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let sig_bytes: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;
    let sig = Signature::from_bytes(&sig_bytes);
    let canonical = canonical_payload_python(&payload)?;
    key.verify(&canonical, &sig)
        .map_err(|_| UpdateError::new(UpdateErrorKind::SignatureInvalid))?;

    let version = payload
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::SignatureInvalid))?
        .to_string();
    let tag = payload
        .get("tag")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::SignatureInvalid))?
        .to_string();
    let artifact_url = payload
        .get("artifact_url")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::SignatureInvalid))?
        .to_string();
    let sha256 = payload
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::FileInfoInvalid))?
        .to_lowercase();
    let size = payload
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| UpdateError::new(UpdateErrorKind::FileInfoInvalid))?;
    if !tag.starts_with('v') || tag[1..] != version {
        return Err(UpdateError::new(UpdateErrorKind::TagInvalid));
    }
    if !is_valid_artifact_url(&artifact_url, &tag) {
        return Err(UpdateError::new(UpdateErrorKind::LocationInvalid));
    }
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::new(UpdateErrorKind::FileInfoInvalid));
    }
    if size == 0 || size > MAX_ARTIFACT_BYTES {
        return Err(UpdateError::new(UpdateErrorKind::SizeOutOfRange));
    }
    check_expiry(&payload)?;

    if !is_newer(&version, current_version)? {
        return Err(UpdateError::NoUpdate);
    }
    Ok(UpdateManifest {
        version,
        tag,
        artifact_url,
        sha256,
        size,
    })
}

/// Enforce the manifest freshness window. A missing timestamp is accepted
/// (older manifests predate it); an unparseable or past timestamp is rejected:
/// silently dropping expiry enforcement would let a stale signed manifest
/// linger forever.
fn check_expiry(payload: &Value) -> Result<(), UpdateError> {
    let Some(expires_at_str) = payload.get("expires_at").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(exp_ts) = parse_rfc3339_timestamp(expires_at_str) else {
        return Err(UpdateError::new(UpdateErrorKind::ExpiryInvalid));
    };
    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if exp_ts <= now_ts {
        return Err(UpdateError::new(UpdateErrorKind::Expired));
    }
    Ok(())
}

fn parse_rfc3339_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    // ASCII only (byte slicing below must not split a character), and only
    // UTC designators: an offset would shift the instant, and silently
    // reading `+09:00` as UTC is how a stale manifest outlives its expiry.
    if s.len() < 19 || !s.is_ascii() {
        return None;
    }
    let rest = &s[19..];
    let rest = match rest.strip_prefix('.') {
        Some(frac) => {
            let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            &frac[digits..]
        }
        None => rest,
    };
    if !matches!(rest, "Z" | "z" | "+00:00" | "-00:00") {
        return None;
    }
    let year: u64 = s[0..4].parse().ok()?;
    let month: u64 = s[5..7].parse().ok()?;
    let day: u64 = s[8..10].parse().ok()?;
    let hour: u64 = s[11..13].parse().ok()?;
    let min: u64 = s[14..16].parse().ok()?;
    let sec: u64 = s[17..19].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    let mut days = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    let days_in_months = if is_leap_year(year) {
        [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for m in 1..month {
        days += days_in_months[m as usize];
    }
    days += day - 1;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_url_is_pinned() {
        assert_eq!(
            expected_artifact_url("v0.2.0"),
            "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe"
        );
        assert!(is_valid_artifact_url(
            "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe",
            "v0.2.0"
        ));
        assert!(!is_valid_artifact_url(
            "https://malicious.example.com/KeyboardChatteringFix.exe",
            "v0.2.0"
        ));
        assert!(!is_valid_artifact_url(
            "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/kcfix-updater.exe",
            "v0.2.0"
        ));
    }

    #[test]
    fn verifies_python_built_fixture_against_embedded_key() {
        // Signed with the real release private key by
        // `scripts/build_update_manifest.py` (v99.0.0 fixture, expires
        // 2036). Proves the Rust verifier accepts Python-built signatures
        // against `UPDATE_PUBLIC_KEY_B64`. Also verified from Python with
        // `scripts/verify_update_manifest.py`.
        let doc = r#"{
    "payload": {
        "artifact_url": "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v99.0.0/KeyboardChatteringFix.exe",
        "expires_at": "2036-10-13T07:13:12Z",
        "sha256": "295b7aeb83e7f557398c922f6f302a9e2f63b107765fcf9ab4f11e7bb0cf9295",
        "size": 1024,
        "tag": "v99.0.0",
        "version": "99.0.0"
    },
    "signature": "YdRBDY39QhI5KexZrkqMW/Wtg0vX+dOI1u0VMIu8dRPa+iSVzO4zRRx/2q4qXB9pl62IWXDcd/ZuaYSjSmtGDQ=="
}"#;
        // Against the current version the fixture is newer: update available.
        let manifest = parse_and_verify_manifest(doc.as_bytes(), crate::config::VERSION).unwrap();
        assert_eq!(manifest.version, "99.0.0");
        assert_eq!(manifest.tag, "v99.0.0");
        assert_eq!(manifest.size, 1024);
        // Against itself it is current: no update.
        let err = parse_and_verify_manifest(doc.as_bytes(), "99.0.0").unwrap_err();
        assert!(matches!(err, UpdateError::NoUpdate));
        // Tampering with one byte breaks the signature.
        let tampered = doc.replace("99.0.0", "99.0.1");
        assert!(parse_and_verify_manifest(tampered.as_bytes(), crate::config::VERSION).is_err());
    }

    #[test]
    fn expiry_timestamp_parsing() {
        assert!(parse_rfc3339_timestamp("2027-09-02T13:41:52Z").is_some());
        assert!(parse_rfc3339_timestamp("not-a-timestamp").is_none());
        assert!(parse_rfc3339_timestamp("2027-13-99T99:99:99Z").is_none());
    }

    #[test]
    fn expiry_timestamp_accepts_only_utc_forms() {
        let base = parse_rfc3339_timestamp("2036-10-13T07:13:12Z").unwrap();
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T07:13:12z"), Some(base));
        assert_eq!(
            parse_rfc3339_timestamp("2036-10-13T07:13:12+00:00"),
            Some(base)
        );
        assert_eq!(
            parse_rfc3339_timestamp("2036-10-13T07:13:12.250Z"),
            Some(base)
        );
        // UG4: a real offset must not be read as UTC; no designator at all
        // is ambiguous too.
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T16:13:12+09:00"), None);
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T07:13:12-05:00"), None);
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T07:13:12"), None);
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T07:13:12.Z"), None);
        // Non-ASCII must be rejected, not panic on a char boundary.
        assert_eq!(parse_rfc3339_timestamp("2036-10-13T07:13:1２Z"), None);
    }

    #[test]
    fn check_expiry_rejects_bad_and_past_timestamps() {
        use serde_json::json;
        // Missing timestamp predates the field: accepted.
        assert!(check_expiry(&json!({"version": "1.0.0"})).is_ok());
        // Well-formed future timestamp: accepted.
        assert!(check_expiry(&json!({"expires_at": "2036-10-13T07:13:12Z"})).is_ok());
        // Past timestamp: expired.
        let err = check_expiry(&json!({"expires_at": "2000-01-01T00:00:00Z"})).unwrap_err();
        assert!(err.kind() == Some(UpdateErrorKind::Expired), "{err}");
        // Unparseable timestamp: fail closed, no silent acceptance.
        let err = check_expiry(&json!({"expires_at": "soon"})).unwrap_err();
        assert!(err.kind() == Some(UpdateErrorKind::ExpiryInvalid), "{err}");
    }
}
