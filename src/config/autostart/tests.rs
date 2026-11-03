//! Windows startup integration: the `HKCU\...\Run` value (normal start) and
//! an optional scheduled task that starts the program as administrator at
//! sign-in (so windows running as administrator are filtered too).
//!
//! Both pass `--autostart`. The two may coexist: the `Run` start then acts
//! as a fallback. While the elevated task is registered (marker value under
//! `HKCU\Software\KeyboardChatteringFix`), a non-elevated automatic start
//! first waits for the elevated one (see `app::bootstrap`), and whichever
//! comes second leaves quietly instead of showing "already running".

use windows::Win32::System::Threading::GetCurrentProcess;

use super::marker::{exe_in_protected_location, is_under_any, protected_roots};
use super::run::{approved_says_disabled, needs_flag_upgrade, quote_run_path, registered_exe};
use super::task::{process_user_sid, task_xml};

#[test]
fn run_path_is_quoted() {
    assert_eq!(
        quote_run_path(r"C:\Program Files\App\app.exe"),
        r#""C:\Program Files\App\app.exe" --autostart"#
    );
    assert_eq!(quote_run_path(r"C:\app.exe"), r#""C:\app.exe" --autostart"#);
}

#[test]
fn only_our_own_old_entry_gets_the_flag() {
    let exe = r"C:\Tools\KeyboardChatteringFix.exe";
    assert!(needs_flag_upgrade(
        r#""C:\Tools\KeyboardChatteringFix.exe""#,
        exe
    ));
    assert!(needs_flag_upgrade(
        r#""c:\tools\keyboardchatteringfix.exe""#,
        exe
    ));
    // Already upgraded.
    assert!(!needs_flag_upgrade(&quote_run_path(exe), exe));
    // Another copy (e.g. a test build) must never take over the entry.
    assert!(!needs_flag_upgrade(
        r#""D:\dev\target\debug\kcfix.exe""#,
        exe
    ));
}

#[test]
fn registered_exe_handles_quoted_and_bare_entries() {
    let exe = r"C:\Program Files\KCFix\kcfix.exe";
    assert_eq!(registered_exe(&quote_run_path(exe)), exe);
    assert_eq!(registered_exe(&format!(" \"{exe}\" ")), exe);
    assert_eq!(
        registered_exe(r"C:\Tools\kcfix.exe --autostart"),
        r"C:\Tools\kcfix.exe"
    );
    assert_eq!(registered_exe(r"C:\Tools\kcfix.exe"), r"C:\Tools\kcfix.exe");
}

#[test]
fn task_manager_disabled_mark_is_the_odd_first_byte() {
    assert!(!approved_says_disabled(&[0x02, 0, 0, 0]));
    assert!(!approved_says_disabled(&[0x06, 0, 0, 0]));
    assert!(approved_says_disabled(&[0x03, 0, 0, 0, 1, 2, 3, 4]));
    assert!(approved_says_disabled(&[0x07]));
    // Missing or empty data: Windows treats the entry as enabled.
    assert!(!approved_says_disabled(&[]));
}

#[test]
fn protected_location_means_inside_a_protected_root() {
    let roots = [
        r"C:\Program Files".to_string(),
        r"C:\Program Files (x86)".to_string(),
        String::new(),
    ];
    assert!(is_under_any(r"C:\Program Files\KCFix\kcfix.exe", &roots));
    assert!(is_under_any(r"c:\program files (x86)\k\k.exe", &roots));
    assert!(is_under_any(r"\\?\C:\Program Files\K\k.exe", &roots));
    // A sibling whose name merely starts the same is not inside.
    assert!(!is_under_any(r"C:\Program Files Evil\k.exe", &roots));
    assert!(!is_under_any(
        r"C:\Users\kim\scoop\apps\keyboard-chattering-fix\current\k.exe",
        &roots
    ));
    // An empty root (lookup failed) never matches everything.
    assert!(!is_under_any(r"D:\Tools\k.exe", &[String::new()]));
}

#[test]
fn known_protected_roots_resolve() {
    let roots = protected_roots();
    assert!(
        roots
            .iter()
            .any(|r| r.to_lowercase().ends_with(r"\windows")),
        "{roots:?}"
    );
    // The test binary lives in the repository, not under Program Files.
    assert!(!exe_in_protected_location());
}

#[test]
fn our_own_user_sid_is_readable() {
    let sid = unsafe { process_user_sid(GetCurrentProcess()) }.expect("own token");
    assert!(sid.starts_with("S-1-"), "{sid}");
}

#[test]
fn task_definition_runs_elevated_on_battery_without_time_limit() {
    let xml = task_xml(r"C:\Tools & Co\kcfix.exe", "S-1-5-21-1-2-3-1001");
    assert!(xml.contains("<Command>C:\\Tools &amp; Co\\kcfix.exe</Command>"));
    assert!(xml.contains("<Arguments>--autostart</Arguments>"));
    // Both the trigger and the principal name the user: a logon trigger
    // without one would start the program for every account.
    assert_eq!(
        xml.matches("<UserId>S-1-5-21-1-2-3-1001</UserId>").count(),
        2
    );
    for needle in [
        "<RunLevel>HighestAvailable</RunLevel>",
        "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
        "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
        "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
        "<LogonType>InteractiveToken</LogonType>",
    ] {
        assert!(xml.contains(needle), "{needle}");
    }
}
