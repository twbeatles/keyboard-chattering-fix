//! Process-wide runtime state shared by the hook, the window, and tests.
//!
//! Everything here is set once during startup (`app::bootstrap`) and then
//! only read or locked — never re-created — so the low-level keyboard hook
//! and the UI thread always observe the same filter state.

use std::{
    sync::{atomic::AtomicBool, Mutex, OnceLock},
    time::Instant,
};

use crate::{config::ValidatedSound, debounce::Debouncer};

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
}

pub(crate) static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
pub(crate) static START: OnceLock<Instant> = OnceLock::new();
/// Local wall-clock time of day (ms since midnight) captured with `START`,
/// so the History tab can show `HH:MM:SS` for monotonic event timestamps.
pub(crate) static START_CLOCK_MS: OnceLock<u64> = OnceLock::new();
/// Whether the keyboard hook is installed. False only when installing it
/// failed at startup (G1), so the UI never claims protection it lacks.
pub(crate) static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
