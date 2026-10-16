use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use tracing::warn;

use super::canonical::canonical_payload_python;
use super::error::UpdateError;
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
        return Err(UpdateError::Message(
            "업데이트 매니페스트가 올바르지 않습니다.".into(),
        ));
    }
    let parsed: Value = serde_json::from_slice(document)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let payload = parsed.get("payload").cloned().ok_or_else(|| {
        UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into())
    })?;
    let signature_b64 = parsed
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into())
        })?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let public = base64::engine::general_purpose::STANDARD
        .decode(UPDATE_PUBLIC_KEY_B64)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let key_bytes: [u8; 32] = public
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let sig_bytes: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;
    let sig = Signature::from_bytes(&sig_bytes);
    let canonical = canonical_payload_python(&payload)?;
    key.verify(&canonical, &sig)
        .map_err(|_| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?;

    let version = payload
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let tag = payload
        .get("tag")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let artifact_url = payload
        .get("artifact_url")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 서명 또는 형식 검증에 실패했습니다.".into()))?
        .to_string();
    let sha256 = payload
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| UpdateError::Message("업데이트 파일 정보가 올바르지 않습니다.".into()))?
        .to_lowercase();
    let size = payload
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| UpdateError::Message("업데이트 파일 정보가 올바르지 않습니다.".into()))?;
    if !tag.starts_with('v') || tag[1..] != version {
        return Err(UpdateError::Message(
            "업데이트 태그 정보가 올바르지 않습니다.".into(),
        ));
    }
    if !is_valid_artifact_url(&artifact_url, &tag) {
        return Err(UpdateError::Message(
            "업데이트 파일 위치가 올바르지 않습니다.".into(),
        ));
    }
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::Message(
            "업데이트 파일 정보가 올바르지 않습니다.".into(),
        ));
    }
    if size == 0 || size > MAX_ARTIFACT_BYTES {
        return Err(UpdateError::Message(
            "업데이트 파일 크기가 허용 범위를 벗어났습니다.".into(),
        ));
    }
    if let Some(expires_at_str) = payload.get("expires_at").and_then(Value::as_str) {
        if let Some(exp_ts) = parse_rfc3339_timestamp(expires_at_str) {
            let now_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if exp_ts <= now_ts {
                return Err(UpdateError::Message(
                    "업데이트 매니페스트가 만료되었습니다.".into(),
                ));
            }
        } else {
            // Fail open (the signature is valid; only our builder mints these),
            // but say so: an unparseable timestamp means the builder changed.
            warn!("update manifest expires_at is not RFC3339; accepting on signature");
        }
    }
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

fn parse_rfc3339_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() < 19 {
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
    fn expiry_timestamp_parsing_documents_fail_open() {
        assert!(parse_rfc3339_timestamp("2027-09-02T13:41:52Z").is_some());
        assert!(parse_rfc3339_timestamp("not-a-timestamp").is_none());
        assert!(parse_rfc3339_timestamp("2027-13-99T99:99:99Z").is_none());
    }
}
