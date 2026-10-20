//! Process-wide runtime state shared by the hook, the window, and tests.
//!
//! Everything here is set once during startup (`app::bootstrap`) and then
//! only read or locked — never re-created — so the low-level keyboard hook
//! and the UI thread always observe the same filter state.

use std::{
    sync::{Mutex, OnceLock, atomic::AtomicBool},
    time::Instant,
};

use crate::{
    config::ValidatedSound,
    debounce::{Debouncer, SLOTS},
    input::hook::InjectQueue,
};

/// Live filter state guarded by a mutex: the hook locks it per keystroke,
/// the UI thread locks it per timer tick and per settings save.
pub(crate) struct Shared {
    pub debouncer: Debouncer,
    pub enabled: bool,
    pub play_sound: bool,
    /// Custom sound validated once at startup/save time, never per keystroke.
    /// The UTF-16 encoding is precomputed so the hook plays it with zero
    /// allocation on its hot path.
    pub sound_file: Option<ValidatedSound>,
    pub exclude_injected: bool,
    /// Protection paused because a listed or full-screen app is in front
    /// (set by the UI's foreground check; `enabled` stays the user's choice).
    pub suspended: bool,
    /// Scan code of each held-back release, to send it again unchanged.
    pub pending_scan: [u16; SLOTS],
    /// Events the hook thread must send (release delay).
    pub inject: InjectQueue,
}

pub(crate) static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
pub(crate) static START: OnceLock<Instant> = OnceLock::new();
/// Local wall-clock time of day (ms since midnight) captured with `START`,
/// so the History tab can show `HH:MM:SS` for monotonic event timestamps.
pub(crate) static START_CLOCK_MS: OnceLock<u64> = OnceLock::new();
/// Whether the keyboard hook is installed. False when installing it failed
/// (G1), so the UI never claims protection it lacks.
pub(crate) static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
