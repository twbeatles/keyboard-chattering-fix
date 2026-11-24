//! Per-key deterioration watch from the persisted day history.
//!
//! Counts are grouped by local calendar day (`day.YYYYMMDD`). The watch
//! compares the last 7 days against the 7 before them: a key whose blocked
//! count jumps has a switch going bad, not a threshold set too low.
//!
//! Pure functions over [`DayEntry`] slices; the UI renders the result.

use crate::config::DayEntry;

/// Days in the recent window and in the baseline window before it.
pub const TREND_WINDOW_DAYS: u64 = 7;
/// A key needs this many blocked presses in the recent window before it
/// counts as deteriorating (below it, the numbers are noise).
pub const DETERIORATE_MIN_BLOCKED: u64 = 20;
/// ... and this multiple of the previous window (a fresh chatter onset
/// counts even with no previous blocks).
pub const DETERIORATE_RATIO: u64 = 3;

/// Day number of a `YYYYMMDD` date (`None` for impossible dates, which a
/// hand-edited settings file may hold). Counted like the history clock so
/// month boundaries just work.
pub fn ymd_to_days(date: u32) -> Option<u64> {
    let y = date / 10000;
    let m = (date / 100) % 100;
    let d = date % 100;
    if !(1..=12).contains(&m) {
        return None;
    }
    let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        _ => 28,
    };
    if d < 1 || d > dim {
        return None;
    }
    // Howard Hinnant's days_from_civil, epoch 1970-01-01.
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468).try_into().ok()
}

/// (presses, blocked) summed over the `window_days` days ending at
/// `end_days` (exclusive of the start), per virtual key. Entries with
/// unreadable dates never match a window, so they are ignored.
pub fn window_sums(history: &[DayEntry], end_days: u64, window_days: u64) -> [(u64, u64); 256] {
    let mut sums = [(0u64, 0u64); 256];
    for entry in history {
        let Some(day) = ymd_to_days(entry.date) else {
            continue;
        };
        if day <= end_days && end_days - day < window_days {
            for vk in 0..256 {
                sums[vk].0 = sums[vk].0.saturating_add(entry.presses[vk]);
                sums[vk].1 = sums[vk].1.saturating_add(entry.blocked[vk]);
            }
        }
    }
    sums
}

/// Whether a key's blocked count calls for a switch check: enough recent
/// blocks, and either new chatter or a several-fold jump.
fn deteriorated(recent_blocked: u64, prev_blocked: u64) -> bool {
    recent_blocked >= DETERIORATE_MIN_BLOCKED
        && (prev_blocked == 0 || recent_blocked >= prev_blocked.saturating_mul(DETERIORATE_RATIO))
}

/// Deteriorated keys as `(vk, recent blocked, previous blocked)`, worst
/// first, at most `max` of them. Empty when `today` is unreadable.
pub fn worst_keys(history: &[DayEntry], today: u32, max: usize) -> Vec<(u8, u64, u64)> {
    let Some(end) = ymd_to_days(today) else {
        return Vec::new();
    };
    let prev_end = end.saturating_sub(TREND_WINDOW_DAYS);
    let recent = window_sums(history, end, TREND_WINDOW_DAYS);
    let prev = window_sums(history, prev_end, TREND_WINDOW_DAYS);
    let mut out: Vec<(u8, u64, u64)> = (0..=255u8)
        .filter_map(|vk| {
            let (rb, pb) = (recent[vk as usize].1, prev[vk as usize].1);
            deteriorated(rb, pb).then_some((vk, rb, pb))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.truncate(max);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: u32, vk: u8, presses: u64, blocked: u64) -> DayEntry {
        let mut e = DayEntry {
            date,
            presses: [0; 256],
            blocked: [0; 256],
        };
        e.presses[vk as usize] = presses;
        e.blocked[vk as usize] = blocked;
        e
    }

    #[test]
    fn ymd_converts_known_dates_and_rejects_impossible_ones() {
        assert_eq!(ymd_to_days(19700101), Some(0));
        // 2024-01-01 is day 19723 (54 years + 13 leap days); Feb 29 adds 59.
        assert_eq!(ymd_to_days(20240229), Some(19723 + 59));
        assert_eq!(ymd_to_days(20261124), ymd_to_days(20261123).map(|d| d + 1));
        for bad in [0, 20261301, 20251131, 20250229, 99999999] {
            assert_eq!(ymd_to_days(bad), None, "{bad}");
        }
    }

    #[test]
    fn window_sums_reach_across_month_boundaries() {
        // Leap March 1st: the trailing 7-day window covers Feb 24-29.
        let history = vec![
            day(20240220, 0x41, 100, 50), // too old
            day(20240224, 0x41, 100, 7),
            day(20240229, 0x41, 100, 5),
            day(99999999, 0x41, 100, 500), // unreadable date: ignored
        ];
        let sums = window_sums(&history, ymd_to_days(20240301).unwrap(), 7);
        assert_eq!((sums[0x41].0, sums[0x41].1), (200, 12));
    }

    #[test]
    fn deterioration_needs_enough_blocks_and_a_jump() {
        assert!(!deteriorated(19, 0));
        assert!(deteriorated(20, 0));
        assert!(deteriorated(20, 6));
        assert!(!deteriorated(20, 7));
        assert!(deteriorated(60, 20));
        assert!(!deteriorated(59, 20));
    }

    #[test]
    fn worst_keys_ranks_and_caps() {
        let history = vec![
            day(20261112, 0x41, 1000, 5),  // previous window only
            day(20261120, 0x41, 1000, 30), // 6x jump
            day(20261120, 0x42, 1000, 25), // smaller jump
            day(20261120, 0x43, 1000, 3),  // below the minimum
        ];
        let worst = worst_keys(&history, 20261124, 10);
        assert_eq!(worst, vec![(0x41, 30, 5), (0x42, 25, 0)]);
        assert_eq!(worst_keys(&history, 20261124, 1).len(), 1);
        assert!(worst_keys(&history, 99999999, 10).is_empty());
    }
}
