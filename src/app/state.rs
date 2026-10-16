//! Process-wide runtime state shared by the hook, the window, and tests.
//!
//! Everything here is set once during startup (`app::bootstrap`) and then
//! only read or locked — never re-created — so the low-level keyboard hook
//! and the UI thread always observe the same filter state.

use std::{
    sync::{atomic::AtomicIsize, Mutex, OnceLock},
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
/// Raw `HHOOK` installed by the bootstrap; unhooked on `WM_DESTROY`.
pub(crate) static HOOK: AtomicIsize = AtomicIsize::new(0);
