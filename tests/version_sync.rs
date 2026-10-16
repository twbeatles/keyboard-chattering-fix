//! Recurrence guard for version drift.
//!
//! The release version lives in two places: `[package] version` in
//! `Cargo.toml` and `config::VERSION` in `src/config.rs`. The tag pipeline
//! (`scripts/build_release.ps1`, `.github/workflows/release.yml`) compares
//! the Git tag against `config::VERSION`, so bumping only `Cargo.toml`
//! would pass local checks and fail the release build. This test checks
//! both against each other inside CI.
//!
//! Release procedure: bump BOTH to the same `major.minor.patch`, then tag
//! `v{VERSION}`. See `docs/UPDATE.md`.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("repo root")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn config_version() -> String {
    let text = read("src/config.rs");
    let marker = "pub const VERSION: &str = \"";
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("VERSION not found in src/config.rs"))
        + marker.len();
    let end = text[start..]
        .find('"')
        .unwrap_or_else(|| panic!("VERSION unterminated in src/config.rs"))
        + start;
    text[start..end].to_string()
}

#[test]
fn version_is_three_numeric_components() {
    let version = config_version();
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "VERSION must be major.minor.patch: {version}"
    );
    for part in parts {
        assert!(
            !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()),
            "VERSION component must be numeric: {version}"
        );
    }
}

#[test]
fn cargo_package_version_matches() {
    let version = config_version();
    let manifest = read("Cargo.toml");
    let expected = format!("version = \"{version}\"");
    assert!(
        manifest.contains(&expected),
        "[package] version in Cargo.toml must be {version}"
    );
}

#[test]
fn license_file_matches_manifests() {
    // README links LICENSE and both crates declare it; a missing file
    // breaks the link and the packaging metadata at once.
    assert!(
        repo_root().join("LICENSE").is_file(),
        "LICENSE file must exist (README links it)"
    );
    let root_manifest = read("Cargo.toml");
    assert!(
        root_manifest.contains("license"),
        "root Cargo.toml must declare a license"
    );
    let helper_manifest = read("crates/kcfix-updater/Cargo.toml");
    assert!(
        helper_manifest.contains("license"),
        "helper Cargo.toml must declare a license"
    );
}

#[test]
fn readme_documents_update_flow() {
    // The tray update entry is undiscoverable without docs; both language
    // sections must describe how to check for updates.
    let readme = read("README.md");
    for needle in [
        "Check for Updates",
        "업데이트 확인",
        "--check-update",
        "docs/UPDATE.md",
    ] {
        assert!(readme.contains(needle), "README.md must mention {needle}");
    }
}

#[test]
fn updater_artifact_name_matches_release_pipeline() {
    // The manifest pin, the build script output name, and the release
    // workflow must all agree on the artifact file name. A rename in one
    // place without the others ships an update the app will reject.
    let manifest_rs = read("src/updater/manifest.rs");
    let build_ps1 = read("scripts/build_release.ps1");
    let release_yml = read(".github/workflows/release.yml");
    for (file, text) in [
        ("src/updater/manifest.rs", manifest_rs.as_str()),
        ("scripts/build_release.ps1", build_ps1.as_str()),
        (".github/workflows/release.yml", release_yml.as_str()),
    ] {
        assert!(
            text.contains("KeyboardChatteringFix.exe"),
            "{file} must reference the KeyboardChatteringFix.exe artifact"
        );
    }
}
