//! Smoke test: the real built executable must start and answer `--version`.
//!
//! This exercises process launch, CLI parsing, and the console-attach path
//! of the actual binary (not just library logic). It never opens the GUI,
//! so it cannot hang the test runner.

use std::process::Command;

#[test]
fn version_flag_reports_cargo_version_and_exits_zero() {
    let exe = env!("CARGO_BIN_EXE_keyboard-chattering-fix");
    let output = Command::new(exe)
        .arg("--version")
        .output()
        .expect("launch the built executable");
    assert!(
        output.status.success(),
        "--version must exit 0, got {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("keyboard-chattering-fix {}", env!("CARGO_PKG_VERSION"));
    assert!(
        stdout.contains(&expected),
        "stdout must contain {expected:?}, got {stdout:?}"
    );
}

#[test]
fn short_version_flag_behaves_like_long_flag() {
    let exe = env!("CARGO_BIN_EXE_keyboard-chattering-fix");
    let output = Command::new(exe)
        .arg("-V")
        .output()
        .expect("launch the built executable");
    assert!(
        output.status.success(),
        "-V must exit 0, got {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("keyboard-chattering-fix {}", env!("CARGO_PKG_VERSION"));
    assert!(
        stdout.contains(&expected),
        "stdout must contain {expected:?}, got {stdout:?}"
    );
}

/// `--check-helper` on a copy of the app with no helper beside it: the
/// update path must report the missing helper and exit 1 (never open the GUI).
#[test]
fn check_helper_reports_missing_helper() {
    let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_keyboard-chattering-fix"));
    let dir = std::env::temp_dir().join(format!("kcfix_smoke_nohelper_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let alone = dir.join("KeyboardChatteringFix.exe");
    std::fs::copy(&exe, &alone).unwrap();
    let output = Command::new(&alone)
        .arg("--check-helper")
        .output()
        .expect("launch the copied executable");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("kcfix-updater.exe"), "{stderr}");
}

/// `--check-helper` with the workspace-built helper beside the app: the
/// helper the next update would stage is found and its stamp accepted.
#[test]
fn check_helper_accepts_the_workspace_helper() {
    let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_keyboard-chattering-fix"));
    let helper = exe.with_file_name("kcfix-updater.exe");
    if !helper.is_file() {
        // `cargo test -p keyboard-chattering-fix` alone does not build it.
        eprintln!("skipped: kcfix-updater.exe not built beside the app");
        return;
    }
    let dir = std::env::temp_dir().join(format!("kcfix_smoke_helper_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = dir.join("KeyboardChatteringFix.exe");
    std::fs::copy(&exe, &app).unwrap();
    std::fs::copy(&helper, dir.join("kcfix-updater.exe")).unwrap();
    let output = Command::new(&app)
        .arg("--check-helper")
        .output()
        .expect("launch the copied executable");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let quad = format!("{}.0", env!("CARGO_PKG_VERSION"));
    assert!(
        stdout.contains("helper ok") && stdout.contains(&quad),
        "{stdout}"
    );
}
