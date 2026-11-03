use std::sync::atomic::Ordering;

use super::nonce::{UPDATE_EXIT_NONCE, update_exit_authenticated};
use super::session::{SessionReply, session_reply};
use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

#[test]
fn session_end_persists_only_when_the_session_really_ends() {
    assert_eq!(session_reply(WM_QUERYENDSESSION, 0), SessionReply::AllowEnd);
    assert_eq!(session_reply(WM_ENDSESSION, 1), SessionReply::PersistNow);
    assert_eq!(session_reply(WM_ENDSESSION, 0), SessionReply::Ignore);
}

#[test]
fn update_exit_needs_this_process_nonce() {
    UPDATE_EXIT_NONCE.store(0x1234_5678_9ABC_DEF0, Ordering::Relaxed);
    assert!(update_exit_authenticated(0x1234_5678_9ABC_DEF0));
    assert!(!update_exit_authenticated(0));
    assert!(!update_exit_authenticated(0xDEAD_BEEF));
    UPDATE_EXIT_NONCE.store(0, Ordering::Relaxed);
    assert!(!update_exit_authenticated(0x1234_5678_9ABC_DEF0));
}
