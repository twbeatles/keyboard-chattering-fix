use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::model::RenderedStats;

pub(super) static LAST_RENDERED: Mutex<Option<RenderedStats>> = Mutex::new(None);

/// Forget the last frame so the next refresh repaints everything
/// (language change).
pub(crate) fn invalidate() {
    *LAST_RENDERED.lock().unwrap() = None;
}

/// Reentrancy guard for the 500 ms stats tick (ISSUE-003). Modal dialogs pump
/// nested messages, so `WM_TIMER` can re-enter `update_stats`; the nested tick
/// is dropped instead of piling up `SetWindowTextW` calls. On drop, ticks
/// slower than the budget are logged to aid hang diagnosis.
pub(super) static STATS_TICK_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Ticks slower than this are reported; the timer fires every 500 ms.
const SLOW_TICK_MS: u128 = 250;

pub(super) struct StatsTickGuard {
    started: std::time::Instant,
}
impl StatsTickGuard {
    pub(super) fn enter() -> Option<StatsTickGuard> {
        if STATS_TICK_ACTIVE.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(StatsTickGuard {
                started: std::time::Instant::now(),
            })
        }
    }
}
impl Drop for StatsTickGuard {
    fn drop(&mut self) {
        STATS_TICK_ACTIVE.store(false, Ordering::SeqCst);
        let elapsed_ms = self.started.elapsed().as_millis();
        if elapsed_ms > SLOW_TICK_MS {
            tracing::warn!(
                elapsed_ms = elapsed_ms,
                "slow stats tick; investigate for UI hang (ISSUE-003)"
            );
            // The GUI has no console: keep the evidence in the file log.
            super::super::diag::log_line(&format!("slow stats tick: {elapsed_ms} ms"));
        }
    }
}
