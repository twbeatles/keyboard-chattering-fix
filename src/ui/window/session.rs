/// How to answer the session-end messages.
use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SessionReply {
    /// `WM_QUERYENDSESSION`: never veto a shutdown.
    AllowEnd,
    /// `WM_ENDSESSION(TRUE)`: the session really ends; persist now.
    PersistNow,
    /// `WM_ENDSESSION(FALSE)`: the shutdown was cancelled.
    Ignore,
}

pub(super) fn session_reply(msg: u32, wparam: usize) -> SessionReply {
    match msg {
        WM_QUERYENDSESSION => SessionReply::AllowEnd,
        WM_ENDSESSION if wparam != 0 => SessionReply::PersistNow,
        _ => SessionReply::Ignore,
    }
}
