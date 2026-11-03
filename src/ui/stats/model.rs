use std::time::Duration;

use super::super::foreground::Pause;

/// Point-in-time copy of everything the readout needs, taken under one
/// short lock so no Win32 call ever runs while holding `SHARED`.
pub(crate) struct StatsSnapshot {
    pub cleaning_mode: bool,
    /// The user's protection switch.
    pub enabled: bool,
    /// Whether the keyboard hook is installed (false only after a failed
    /// install, G1).
    pub hook_active: bool,
    pub total_presses: u64,
    pub blocked_presses: u64,
    pub last_blocked: Option<(u8, Duration)>,
    pub top_chattered: Option<(u8, u64, f64)>,
    /// Paused for an app in front (`foreground`).
    pub pause: Option<Pause>,
    /// A window running as administrator is in front (we are not).
    pub elevated_foreground: bool,
    /// Local time the watchdog last restarted the hook.
    pub hook_restarted_at: Option<String>,
}

/// Fully rendered readout for one refresh, built with no lock held and no
/// window touched.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedStats {
    pub cleaning: bool,
    /// The switch position, mirrored into the toggle (and saved from it).
    pub enabled: bool,
    /// Whether input is actually filtered: switch on AND hook installed.
    /// Drives the badge and tray; never fed back into the switch, or a
    /// failed hook would get `bFilterEnabled=0` saved by the next Save.
    pub active: bool,
    pub paused: bool,
    pub badge: String,
    /// Home note under the tip: why protection is paused, an elevated
    /// window in front, or a hook restart.
    pub note: String,
    pub total: String,
    pub blocked: String,
    pub ratio: String,
    pub last: String,
    pub top: String,
}

/// Which parts of the window must be refreshed between two frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatsChanges {
    pub status: bool,
    pub total: bool,
    pub blocked: bool,
    pub ratio: bool,
    pub last: bool,
    pub top: bool,
    pub note: bool,
}

impl StatsChanges {
    pub(super) const ALL: StatsChanges = StatsChanges {
        status: true,
        total: true,
        blocked: true,
        ratio: true,
        last: true,
        top: true,
        note: true,
    };
}
