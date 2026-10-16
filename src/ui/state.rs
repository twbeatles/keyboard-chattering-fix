//! UI-owned state: the active language and the last saved settings.
//!
//! `SAVED` mirrors what is on screen (updated by `on_save`, flushed on
//! `WM_DESTROY`); the live filter state itself lives in `app::state`.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex, OnceLock,
};

use crate::{config::Settings, i18n::Language};

pub(crate) static LANG: AtomicU8 = AtomicU8::new(0);
pub(crate) static SAVED: OnceLock<Mutex<Settings>> = OnceLock::new();

pub(crate) fn current_lang() -> Language {
    if LANG.load(Ordering::Relaxed) == 1 {
        Language::En
    } else {
        Language::Ko
    }
}
