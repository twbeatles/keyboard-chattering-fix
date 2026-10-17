//! UI-owned state: the active language and the last saved settings.
//!
//! `SAVED` mirrors what was last written to disk (updated by `on_save` and
//! the language switch, flushed with live counters on `WM_DESTROY`); the
//! live filter state itself lives in `app::state`.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, OnceLock,
};

use crate::{config::Settings, i18n::Language};

/// Index into `Language::ALL` of the language being shown.
static LANG: AtomicUsize = AtomicUsize::new(0);
pub(crate) static SAVED: OnceLock<Mutex<Settings>> = OnceLock::new();

pub(crate) fn current_lang() -> Language {
    Language::from_index(LANG.load(Ordering::Relaxed)).unwrap_or(Language::FALLBACK)
}

pub(crate) fn set_lang(lang: Language) {
    LANG.store(lang.index(), Ordering::Relaxed);
}
