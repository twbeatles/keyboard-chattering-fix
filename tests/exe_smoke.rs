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
