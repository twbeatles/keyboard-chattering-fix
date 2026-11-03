//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};

use super::page::Page;

pub(super) static PAGE: AtomicU8 = AtomicU8::new(0);
/// Whether the cleaning overlay is what the window currently shows.
pub(super) static SHOWING_CLEANING: AtomicBool = AtomicBool::new(false);
pub(super) static DPI: AtomicI32 = AtomicI32::new(96);

pub(crate) fn current_page() -> Page {
    Page::from_index(PAGE.load(Ordering::Relaxed))
}

pub(crate) fn set_current_page(page: Page) {
    PAGE.store(page as u8, Ordering::Relaxed);
}

pub(crate) fn showing_cleaning() -> bool {
    SHOWING_CLEANING.load(Ordering::Relaxed)
}

pub(crate) fn set_showing_cleaning(on: bool) {
    SHOWING_CLEANING.store(on, Ordering::Relaxed);
}

pub(crate) fn set_dpi(dpi: i32) {
    DPI.store(dpi.max(96), Ordering::Relaxed);
}

/// Scale a logical (96-DPI) length to device pixels.
pub(crate) fn sc(v: i32) -> i32 {
    let dpi = DPI.load(Ordering::Relaxed);
    (v * dpi + 48) / 96
}
