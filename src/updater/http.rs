use std::io::Read;

use sha2::{Digest, Sha256};

use super::error::{UpdateError, UpdateErrorKind};
use super::manifest::parse_and_verify_manifest;
use super::model::{
    UpdateManifest, HTTP_CONNECT_TIMEOUT, HTTP_READ_TIMEOUT, HTTP_TOTAL_TIMEOUT, MANIFEST_URL,
    USER_AGENT,
};
use crate::config::VERSION;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

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

pub fn download_and_verify(
    manifest: &UpdateManifest,
    dest: &std::path::Path,
) -> Result<(), UpdateError> {
    let response = http_agent()
        .get(&manifest.artifact_url)
        .call()
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::Download, err))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(manifest.size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::Download, err))?;
    if bytes.len() as u64 != manifest.size {
        return Err(UpdateError::new(UpdateErrorKind::SizeOutOfRange));
    }
    if sha256_hex(&bytes) != manifest.sha256 {
        return Err(UpdateError::new(UpdateErrorKind::HashMismatch));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| UpdateError::with_detail(UpdateErrorKind::SaveFailed, err))?;
    }
    std::fs::write(dest, bytes)
        .map_err(|err| UpdateError::with_detail(UpdateErrorKind::SaveFailed, err))?;
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
