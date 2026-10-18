use std::io::{Read, Write};

use sha2::{Digest, Sha256};

use super::error::{UpdateError, UpdateErrorKind};
use super::manifest::parse_and_verify_manifest;
use super::model::{
    UpdateManifest, HTTP_CONNECT_TIMEOUT, HTTP_READ_TIMEOUT, HTTP_TOTAL_TIMEOUT, MANIFEST_URL,
    USER_AGENT,
};
use crate::config::VERSION;

fn http_agent() -> ureq::Agent {
    let config = ureq::config::Config::builder()
        .timeout_connect(Some(HTTP_CONNECT_TIMEOUT))
        .timeout_recv_response(Some(HTTP_READ_TIMEOUT))
        .timeout_recv_body(Some(HTTP_READ_TIMEOUT))
        .timeout_global(Some(HTTP_TOTAL_TIMEOUT))
        .user_agent(USER_AGENT)
        .build();
    ureq::Agent::new_with_config(config)
}

/// Stream `reader` into `dest` while hashing: constant memory regardless of
/// artifact size (PERF-001). Returns the received byte count and hex digest.
fn stream_hashed(reader: impl Read, dest: &mut impl Write) -> std::io::Result<(u64, String)> {
    let mut reader = reader;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        received += n as u64;
        hasher.update(&buf[..n]);
        dest.write_all(&buf[..n])?;
    }
    Ok((received, hex::encode(hasher.finalize())))
}

pub fn download_and_verify(
    manifest: &UpdateManifest,
    dest: &std::path::Path,
) -> Result<(), UpdateError> {
    let response = http_agent()
        .get(&manifest.artifact_url)
        .call()
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::Download, err))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| UpdateError::with_detail(UpdateErrorKind::SaveFailed, err))?;
    }
    let mut file = std::fs::File::create(dest)
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::SaveFailed, err))?;
    let (received, digest) = stream_hashed(
        response
            .into_body()
            .into_reader()
            .take(manifest.size.saturating_add(1)),
        &mut file,
    )
    .map_err(|err| UpdateError::with_detail(UpdateErrorKind::Download, err))?;
    drop(file);
    if received != manifest.size {
        let _ = std::fs::remove_file(dest);
        return Err(UpdateError::new(UpdateErrorKind::SizeOutOfRange));
    }
    if digest != manifest.sha256 {
        let _ = std::fs::remove_file(dest);
        return Err(UpdateError::new(UpdateErrorKind::HashMismatch));
    }
    Ok(())
}

pub fn check_for_update() -> Result<UpdateManifest, UpdateError> {
    let body = http_agent()
        .get(MANIFEST_URL)
        .call()
        .map_err(|err| match &err {
            // No release published yet (e.g. before the first GitHub
            // Release): say so instead of a generic download failure.
            ureq::Error::StatusCode(404) => UpdateError::new(UpdateErrorKind::NotPublished),
            _ => UpdateError::with_detail(UpdateErrorKind::ManifestDownload, err),
        })?
        .into_body()
        .read_to_string()
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::ManifestDownload, err))?;
    parse_and_verify_manifest(body.as_bytes(), VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_hashed_writes_and_hashes_incrementally() {
        // Multi-chunk input: bytes pass through untouched and the digest is
        // deterministic across runs.
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        let (n, digest) = stream_hashed(&data[..], &mut out).unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(out, data);
        let mut out2 = Vec::new();
        let (_, digest2) = stream_hashed(&data[..], &mut out2).unwrap();
        assert_eq!(digest, digest2);
        // Empty input yields the well-known empty digest.
        let mut out = Vec::new();
        let (n, digest) = stream_hashed(&[][..], &mut out).unwrap();
        assert_eq!(n, 0);
        assert!(out.is_empty());
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
