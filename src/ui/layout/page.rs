//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use super::super::ids::*;

/// Logical client size of the main window (scaled by DPI at creation).
pub(crate) const CLIENT_W: i32 = 480;
pub(crate) const CLIENT_H: i32 = 742;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    Home = 0,
    History = 1,
    Settings = 2,
    Advanced = 3,
    Keys = 4,
}

impl Page {
    pub(crate) const ALL: [Page; 5] = [
        Page::Home,
        Page::History,
        Page::Settings,
        Page::Advanced,
        Page::Keys,
    ];

    pub(super) fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub(super) fn from_index(i: u8) -> Page {
        match i {
            1 => Page::History,
            2 => Page::Settings,
            3 => Page::Advanced,
            4 => Page::Keys,
            _ => Page::Home,
        }
    }

    /// The tab button that selects this page (the ids follow the page
    /// order, not the on-screen order of the tabs).
    pub(crate) fn tab_id(self) -> i32 {
        ID_TAB_HOME + self as i32
    }

    pub(crate) fn from_tab_id(id: i32) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.tab_id() == id)
    }
}

// Visibility scopes (bit set). A control is shown when the current page's
// bit is set, or — while cleaning mode is active — when CLEANING is set.
pub(crate) const HOME: u8 = 1;
pub(crate) const HISTORY: u8 = 2;
pub(crate) const SETTINGS: u8 = 4;
pub(crate) const ADVANCED: u8 = 8;
pub(crate) const KEYS: u8 = 16;
pub(crate) const CLEANING: u8 = 32;
/// Every page, hidden while cleaning (tabs).
pub(crate) const PAGES: u8 = HOME | HISTORY | SETTINGS | ADVANCED | KEYS;
/// Always visible (header, footer).
pub(crate) const EVERY: u8 = PAGES | CLEANING;
