use std::sync::atomic::Ordering;

use super::super::model::UPDATE_IN_PROGRESS;

pub fn try_begin_update() -> bool {
    UPDATE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn end_update() {
    UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// Whether an update is running: the menu bar and tray menu grey out their
/// "Check for updates" item meanwhile (the flow itself guards with
/// [`try_begin_update`]).
pub fn update_in_progress() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}
