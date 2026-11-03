//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use super::kinds::Spec;
use super::page::{CLEANING, Page};
use super::specs::SPECS;

/// Card backgrounds painted for each page (logical rects).
pub(crate) fn cards(page: Page, cleaning: bool) -> &'static [(i32, i32, i32, i32)] {
    if cleaning {
        return &[(16, 118, 448, 220)];
    }
    match page {
        Page::Home => &[(16, 118, 448, 96), (16, 224, 448, 150), (16, 384, 448, 82)],
        Page::History => &[],
        Page::Settings => &[
            (16, 118, 448, 88),
            (16, 214, 448, 109),
            (16, 331, 448, 154),
            (16, 493, 448, 103),
        ],
        Page::Advanced => &[(16, 118, 448, 248), (16, 374, 448, 73), (16, 455, 448, 142)],
        Page::Keys => &[(16, 118, 448, 130), (16, 256, 448, 346)],
    }
}

pub(crate) fn spec(id: i32) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

pub(crate) fn is_visible(scope: u8, page: Page, cleaning: bool) -> bool {
    if cleaning {
        scope & CLEANING != 0
    } else {
        scope & page.bit() != 0
    }
}

// --- Runtime state (UI thread) -------------------------------------------
