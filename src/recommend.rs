//! Per-key threshold recommendation from the measured gap histogram.
//!
//! Chatter gaps cluster at a few milliseconds, while a person needs tens of
//! milliseconds to release and press the same key again. When the two
//! clusters are separated by an empty stretch, a threshold just above the
//! chatter cluster blocks every bounce without touching real presses. When
//! they run into each other, no threshold can tell them apart and the
//! switch itself needs attention.
//!
//! Pure functions over [`GapHistogram`]; the UI renders the result.

use crate::debounce::{GapHistogram, GAP_BUCKETS, GAP_BUCKET_MS};

/// Measured gaps needed before a recommendation is made.
pub const MIN_SAMPLES: u32 = 20;
/// A chatter cluster must start below this bucket (25 ms): nobody releases
/// and re-presses one key that fast on purpose.
const CHATTER_START_BUCKETS: usize = 5;
/// Empty buckets (10 ms) that separate chatter from real presses.
const EMPTY_RUN: usize = 2;
/// The chatter cluster must end below this bucket (150 ms).
const SEARCH_END: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// Too few presses of this key so far.
    NeedMoreData { have: u32, need: u32 },
    /// No gap fast enough to be chatter: the current threshold is fine.
    NoChatter,
    /// Block below `ms`: every chatter gap was under `chatter_below_ms`, and
    /// real presses started at `normal_from_ms` (when any were measured
    /// under 200 ms; `Some(200)` means "200 ms or more").
    Suggest {
        ms: u64,
        chatter_below_ms: u64,
        normal_from_ms: Option<u64>,
    },
    /// Chatter and real presses overlap: clean or replace the switch.
    Overlap,
}

pub fn recommend(h: &GapHistogram) -> Advice {
    let total = h.iter().fold(0u32, |a, &n| a.saturating_add(n));
    if total < MIN_SAMPLES {
        return Advice::NeedMoreData {
            have: total,
            need: MIN_SAMPLES,
        };
    }
    let Some(first) = h[..CHATTER_START_BUCKETS].iter().position(|&n| n > 0) else {
        return Advice::NoChatter;
    };
    // Walk the chatter cluster until EMPTY_RUN empty buckets in a row.
    // The overflow bucket (200 ms+) never belongs to the cluster.
    let mut last = first;
    let mut separated = false;
    for (i, &n) in h.iter().enumerate().take(GAP_BUCKETS - 1).skip(first + 1) {
        if n > 0 {
            last = i;
        } else if i - last >= EMPTY_RUN {
            separated = true;
            break;
        }
    }
    if !separated || last >= SEARCH_END {
        return Advice::Overlap;
    }
    let normal_from_ms = h
        .iter()
        .enumerate()
        .skip(last + 1)
        .find(|&(_, &n)| n > 0)
        .map(|(i, _)| i as u64 * GAP_BUCKET_MS);
    let chatter_below_ms = (last as u64 + 1) * GAP_BUCKET_MS;
    Advice::Suggest {
        ms: chatter_below_ms + GAP_BUCKET_MS,
        chatter_below_ms,
        normal_from_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Histogram from (gap in ms, count) pairs.
    fn hist(samples: &[(u64, u32)]) -> GapHistogram {
        let mut h = [0; GAP_BUCKETS];
        for &(gap, n) in samples {
            let i = ((gap / GAP_BUCKET_MS) as usize).min(GAP_BUCKETS - 1);
            h[i] += n;
        }
        h
    }

    #[test]
    fn too_few_samples_ask_for_more() {
        let h = hist(&[(3, 5), (80, 14)]);
        assert_eq!(
            recommend(&h),
            Advice::NeedMoreData {
                have: 19,
                need: MIN_SAMPLES
            }
        );
    }

    #[test]
    fn no_fast_gap_means_no_chatter() {
        // Fast typist: 30 ms is quick, but not chatter-fast.
        let h = hist(&[(30, 10), (60, 20), (400, 50)]);
        assert_eq!(recommend(&h), Advice::NoChatter);
    }

    #[test]
    fn separated_clusters_suggest_just_above_the_chatter() {
        // Chatter at 2–17 ms, real presses from 45 ms.
        let h = hist(&[(2, 4), (8, 6), (17, 2), (45, 3), (90, 10), (600, 40)]);
        assert_eq!(
            recommend(&h),
            Advice::Suggest {
                ms: 25,
                chatter_below_ms: 20,
                normal_from_ms: Some(45),
            }
        );
    }

    #[test]
    fn suggestion_stays_below_the_first_real_press() {
        // Chatter at 10–14 ms, the first real press at 30 ms.
        let h = hist(&[(12, 3), (30, 5), (500, 20)]);
        match recommend(&h) {
            Advice::Suggest {
                ms, normal_from_ms, ..
            } => {
                assert_eq!(ms, 20);
                assert!(ms < normal_from_ms.unwrap());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_slow_real_presses_report_the_overflow_bucket() {
        let h = hist(&[(1, 3), (900, 30)]);
        assert_eq!(
            recommend(&h),
            Advice::Suggest {
                ms: 10,
                chatter_below_ms: 5,
                normal_from_ms: Some(200),
            }
        );
    }

    #[test]
    fn continuous_gaps_from_chatter_to_typing_overlap() {
        // Every 5 ms bucket from 0 to 195 ms has samples: no separation.
        let samples: Vec<(u64, u32)> = (0..40).map(|i| (i * 5, 1)).collect();
        assert_eq!(recommend(&hist(&samples)), Advice::Overlap);
    }

    #[test]
    fn chatter_cluster_reaching_past_150_ms_overlaps() {
        let samples: Vec<(u64, u32)> = (0..32).map(|i| (i * 5, 1)).collect();
        assert_eq!(recommend(&hist(&samples)), Advice::Overlap);
    }

    #[test]
    fn single_empty_bucket_is_not_a_separation() {
        // 0–24 ms busy, 25–29 ms empty, 30–34 ms busy, then a clean gap.
        let h = hist(&[
            (0, 5),
            (5, 5),
            (10, 5),
            (15, 2),
            (20, 1),
            (30, 2),
            (300, 10),
        ]);
        match recommend(&h) {
            Advice::Suggest {
                chatter_below_ms, ..
            } => assert_eq!(chatter_below_ms, 35),
            other => panic!("{other:?}"),
        }
    }
}
