use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::super::model::{STAGING_SEQ, StagedUpdate};
use super::guard::{end_update, try_begin_update};
use super::helper::{
    BinaryStamp, HELPER_FILENAME, HELPER_PRODUCT_NAME, file_stamp, helper_compatible,
    resolve_helper,
};
use super::launch::{HELPER_EXIT_TIMEOUT_SECS, helper_args, relaunch_args_from};
use super::stage::{cleanup_staging_dir, is_staging_file_name, unique_staging_path};

/// Serializes tests that mutate the process-global single-flight flag,
/// since the test runner executes test fns on parallel threads.
static FLAG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn relaunch_args_keep_launch_flags_only() {
    assert_eq!(
        relaunch_args_from(["--minimized"]),
        vec!["--minimized".to_string()]
    );
    assert!(relaunch_args_from(["--version"]).is_empty());
    assert!(relaunch_args_from(["--check-update"]).is_empty());
    assert!(relaunch_args_from(Vec::<&str>::new()).is_empty());
}

#[test]
fn unique_staging_paths_differ() {
    let left = unique_staging_path("KeyboardChatteringFix_update", "0.2.0");
    let right = unique_staging_path("KeyboardChatteringFix_update", "0.2.0");
    assert_ne!(left, right);
    assert!(
        left.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("0.2.0")
    );
}

#[test]
fn cleanup_keeps_fresh_staging_and_drops_only_old() {
    let dir = std::env::temp_dir().join(format!(
        "kcfix_staging_age_{}_{}",
        std::process::id(),
        STAGING_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fresh = dir.join("keyboardchatteringfix_update_0.9.9_1_2_3.exe");
    let other = dir.join("unrelated.exe");
    std::fs::write(&fresh, b"fake").unwrap();
    std::fs::write(&other, b"fake").unwrap();
    // An hour's grace: a fresh in-flight file survives (ISSUE-004).
    cleanup_staging_dir(&dir, Duration::from_secs(3600));
    assert!(fresh.exists());
    assert!(other.exists());
    // Zero grace: the leftover is recognized and removed, others kept.
    cleanup_staging_dir(&dir, Duration::ZERO);
    assert!(!fresh.exists());
    assert!(other.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_helper_rejects_non_mz_and_accepts_mz() {
    let dir = std::env::temp_dir().join(format!("kcfix_helper_resolve_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake_app = dir.join("app.exe");
    std::fs::write(&fake_app, b"MZ-app").unwrap();
    assert!(resolve_helper(&fake_app).is_err());

    let helper = dir.join(HELPER_FILENAME);
    std::fs::write(&helper, b"MZ-helper-bytes").unwrap();
    let found = resolve_helper(&fake_app).unwrap();
    assert_eq!(found, helper);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn try_begin_update_is_single_flight() {
    let _lock = FLAG_TEST_LOCK.lock().unwrap();
    end_update();
    assert!(try_begin_update());
    assert!(!try_begin_update());
    end_update();
    assert!(try_begin_update());
    end_update();
}

#[test]
fn try_begin_update_parallel_exactly_one_winner() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _lock = FLAG_TEST_LOCK.lock().unwrap();
    end_update();
    let winners = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                if try_begin_update() {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
    });
    assert_eq!(winners.load(Ordering::SeqCst), 1);
    end_update();
}

#[test]
fn file_stamp_reads_version_and_product_name() {
    // kernel32.dll always carries version info: a numeric quad plus a
    // product name that is NOT the kcfix helper.
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let stamp = file_stamp(&system.join("System32").join("kernel32.dll"))
        .expect("kernel32.dll should carry a version stamp");
    let parts: Vec<&str> = stamp.version.split('.').collect();
    assert_eq!(parts.len(), 4);
    assert!(
        parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    );
    let name = stamp.product_name.as_deref().expect("product name");
    assert!(!name.is_empty());
    // A foreign stamped binary is never accepted as the helper.
    assert!(!helper_compatible(Some(&stamp), crate::config::VERSION));
}

#[test]
fn file_stamp_returns_none_without_stamp() {
    let dir = std::env::temp_dir().join(format!("kcfix_version_none_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.bin");
    std::fs::write(&junk, b"not-an-executable").unwrap();
    assert_eq!(file_stamp(&junk), None);
    let _ = std::fs::remove_dir_all(&dir);
}

fn helper_stamp(version: &str) -> BinaryStamp {
    BinaryStamp {
        version: version.to_string(),
        product_name: Some(HELPER_PRODUCT_NAME.to_string()),
    }
}

#[test]
fn helper_compatible_accepts_older_supported_helpers() {
    // Unstamped binaries predate stamping: accepted for upgrade compatibility.
    assert!(helper_compatible(None, "0.3.0"));
    // Same version, plain or quad.
    assert!(helper_compatible(Some(&helper_stamp("0.3.0.0")), "0.3.0"));
    // ISSUE-001: the helper left over from the previous release must keep
    // working after the app itself was updated — repeatedly.
    assert!(helper_compatible(Some(&helper_stamp("0.2.0.0")), "0.3.0"));
    assert!(helper_compatible(Some(&helper_stamp("0.2.0.0")), "1.4.2"));
}

#[test]
fn helper_compatible_rejects_out_of_range_or_foreign() {
    // Older than the CLI contract floor.
    assert!(!helper_compatible(Some(&helper_stamp("0.1.9.0")), "0.3.0"));
    // Newer than the app: a helper from the future is not trusted.
    assert!(!helper_compatible(Some(&helper_stamp("0.4.0.0")), "0.3.0"));
    // Right version, wrong product.
    let foreign = BinaryStamp {
        version: "0.3.0.0".to_string(),
        product_name: Some("Something Else".to_string()),
    };
    assert!(!helper_compatible(Some(&foreign), "0.3.0"));
    let nameless = BinaryStamp {
        version: "0.3.0.0".to_string(),
        product_name: None,
    };
    assert!(!helper_compatible(Some(&nameless), "0.3.0"));
}

#[test]
fn helper_args_pass_exit_timeout_and_hash() {
    let staged = StagedUpdate {
        helper: PathBuf::from(r"C:\t\h.exe"),
        current_exe: PathBuf::from(r"C:\app\a.exe"),
        replacement: PathBuf::from(r"C:\t\r.exe"),
        sha256: "ab".repeat(32),
        relaunch_args: vec!["--minimized".to_string()],
    };
    let args: Vec<String> = helper_args(&staged, 42)
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let after = |flag: &str| {
        let i = args.iter().position(|a| a == flag).expect(flag);
        args[i + 1].clone()
    };
    assert_eq!(after("--pid"), "42");
    assert_eq!(after("--current"), r"C:\app\a.exe");
    assert_eq!(after("--replacement"), r"C:\t\r.exe");
    // ISSUE-003: never fall back to the helper's 30 s default.
    assert_eq!(
        after("--timeout-secs"),
        HELPER_EXIT_TIMEOUT_SECS.to_string()
    );
    const { assert!(HELPER_EXIT_TIMEOUT_SECS >= 120) };
    assert_eq!(after("--sha256"), "ab".repeat(32));
    assert!(args.iter().any(|a| a == "--relaunch-arg=--minimized"));
    // Only flags HELPER_MIN_VERSION understands: no language flag.
    assert!(!args.iter().any(|a| a == "--lang"));
}

#[test]
fn staging_leftovers_are_recognized_by_name_only() {
    assert!(is_staging_file_name(
        "kcfix-updater_helper_1234_1700000000_0.exe"
    ));
    assert!(is_staging_file_name(
        "KeyboardChatteringFix_update_0.3.0_1_2_3.exe"
    ));
    assert!(!is_staging_file_name("kcfix-updater.exe"));
    assert!(!is_staging_file_name("KeyboardChatteringFix.exe"));
    assert!(!is_staging_file_name("kcfix-updater_helper_1.txt"));
    // The real staging generator matches the recognizer.
    let made = unique_staging_path("kcfix-updater", "helper");
    assert!(is_staging_file_name(
        &made.file_name().unwrap().to_string_lossy()
    ));
    let made = unique_staging_path("KeyboardChatteringFix_update", "0.3.0");
    assert!(is_staging_file_name(
        &made.file_name().unwrap().to_string_lossy()
    ));
}
