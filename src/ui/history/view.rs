//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HistoryView {
    Blocked,
    All,
    Keys,
}

impl HistoryView {
    pub(super) fn from_index(i: isize) -> Self {
        match i {
            1 => HistoryView::All,
            2 => HistoryView::Keys,
            _ => HistoryView::Blocked,
        }
    }

    pub(super) fn index(self) -> usize {
        self as usize
    }
}

/// One list row: its cell texts and whether to draw it in the warning color.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub cells: Vec<String>,
    pub highlight: bool,
    /// The key the row is about (double-click opens it in the Keys tab).
    pub vk: Option<u8>,
}
