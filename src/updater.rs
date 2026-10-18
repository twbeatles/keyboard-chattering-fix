// Pure-move split of the update client (SRP).
//
// - `error`: `UpdateError`
// - `model`: manifest/staged types + endpoint/size/timeout constants
// - `version`: version-tuple parsing + `is_newer`
// - `canonical`: canonical JSON payload encoding (Python-compatible)
// - `manifest`: signed-manifest fetch-shape verification
// - `http`: download/verify + update check over HTTP
// - `staging`: single-flight guard, helper resolution, staging, launch
//
// NOTE: this is a binary crate, so `pub use` here cannot export an API the
// way the reference library crate does; the hub is kept for structural
// parity (same module split, same public surface) and only a subset is
// exercised through these paths.
#![allow(unused_imports)]

// Security model (same as the reference implementation):
// `update.json` is Ed25519-signed, the artifact URL is pinned to this
// repository's Releases, and the staged bytes are SHA-256 verified both at
// download time and again by the helper immediately before the swap.
mod canonical;
mod error;
mod http;
mod manifest;
mod model;
mod staging;
mod version;

pub use canonical::{canonical_payload, canonical_payload_python};
pub use error::{UpdateError, UpdateErrorKind};
pub use http::{check_for_update, download_and_verify};
pub use manifest::{expected_artifact_url, is_valid_artifact_url, parse_and_verify_manifest};
pub use model::{StagedUpdate, UpdateManifest, MANIFEST_URL, RELEASE_DOWNLOAD_PREFIX, USER_AGENT};
pub use staging::{
    apply_update, cleanup_stale_staging, describe_helper, discard_staged, end_update, helper_args,
    launch_helper, prepare_update, relaunch_args_from, resolve_helper, stage_helper,
    try_begin_update, unique_staging_path, unix_now, update_in_progress,
};
pub use version::{is_newer, version_tuple};
