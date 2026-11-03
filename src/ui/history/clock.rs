//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

use crate::app::state::START;
use std::time::Duration;

pub(super) const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day number from [`days_from_civil`].
pub(super) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Turns the monotonic event times (time since startup) into local
/// wall-clock times. Built from the clock *now* on every render, so the
/// date stays right across midnight, days of uptime, and clock changes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    /// Local time (ms since 1970-01-01 00:00, local) at startup.
    base_ms: i64,
    /// Today's local day number.
    today: i64,
}

impl Clock {
    /// `now_local_ms` and `elapsed` (time since startup) read together.
    pub(crate) fn new(now_local_ms: i64, elapsed: Duration) -> Self {
        Self {
            base_ms: now_local_ms - elapsed.as_millis() as i64,
            today: now_local_ms.div_euclid(DAY_MS),
        }
    }

    /// The local clock right now.
    pub(super) fn now() -> Self {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let day = days_from_civil(i64::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay));
        let ms = ((i64::from(t.wHour) * 60 + i64::from(t.wMinute)) * 60 + i64::from(t.wSecond))
            * 1000
            + i64::from(t.wMilliseconds);
        let elapsed = START.get().map(|s| s.elapsed()).unwrap_or_default();
        Self::new(day * DAY_MS + ms, elapsed)
    }

    pub(super) fn parts(&self, t: Duration) -> (i64, i64) {
        let local = self.base_ms + t.as_millis() as i64;
        (local.div_euclid(DAY_MS), local.rem_euclid(DAY_MS))
    }

    /// List cell: `HH:MM:SS.mmm` today, `MM-DD HH:MM:SS` on an earlier day.
    pub(crate) fn list_time(&self, t: Duration) -> String {
        let (day, ms) = self.parts(t);
        let (h, m, sec) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60);
        if day == self.today {
            format!("{h:02}:{m:02}:{sec:02}.{:03}", ms % 1000)
        } else {
            let (_, month, d) = civil_from_days(day);
            format!("{month:02}-{d:02} {h:02}:{m:02}:{sec:02}")
        }
    }

    /// CSV cell: `YYYY-MM-DD HH:MM:SS.mmm`, always with the date.
    pub(crate) fn full_time(&self, t: Duration) -> String {
        let (day, ms) = self.parts(t);
        let (y, month, d) = civil_from_days(day);
        format!(
            "{y:04}-{month:02}-{d:02} {:02}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        )
    }
}
