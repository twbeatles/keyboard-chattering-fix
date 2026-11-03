use super::super::foreground::Pause;
use super::super::keys::vk_name;
use super::model::{RenderedStats, StatsChanges, StatsSnapshot};
use crate::i18n::{Strings, fill};

/// `1234567` -> `1,234,567`, so large counters stay readable.
pub(crate) fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Pure rendering: snapshot plus strings in, control texts out.
/// Unit-testable without windows, hooks, or locks.
pub(crate) fn render_stats(s: &Strings, snap: &StatsSnapshot) -> RenderedStats {
    let ratio = if snap.total_presses == 0 {
        0.0
    } else {
        snap.blocked_presses as f64 * 100.0 / snap.total_presses as f64
    };
    let protecting = snap.enabled && snap.hook_active;
    let paused = protecting && snap.pause.is_some();
    let note = match (
        &snap.pause,
        snap.elevated_foreground,
        &snap.hook_restarted_at,
    ) {
        (Some(Pause::Timed(until)), _, _) => fill(s.note_paused_until_fmt, &[until]),
        (Some(Pause::App(exe)), _, _) => fill(s.note_suspended_fmt, &[exe]),
        (Some(Pause::Fullscreen(exe)), _, _) => fill(s.note_fullscreen_fmt, &[exe]),
        (None, true, _) if protecting => s.note_elevated.to_string(),
        (None, _, Some(at)) => fill(s.note_hook_restarted_fmt, &[at]),
        _ => String::new(),
    };
    RenderedStats {
        cleaning: snap.cleaning_mode,
        enabled: snap.enabled,
        active: protecting && !paused,
        paused,
        badge: (if paused {
            s.status_paused
        } else if protecting {
            s.status_on
        } else {
            s.status_off
        })
        .to_string(),
        note,
        total: group_thousands(snap.total_presses),
        blocked: group_thousands(snap.blocked_presses),
        ratio: format!("{ratio:.1}%"),
        last: match snap.last_blocked {
            Some((vk, gap)) => fill(
                s.last_blocked_fmt,
                &[&vk_name(vk), &gap.as_millis().to_string()],
            ),
            None => s.last_blocked_none.to_string(),
        },
        top: match snap.top_chattered {
            Some((vk, blocked, ratio)) => fill(
                s.top_chattered_fmt,
                &[
                    &vk_name(vk),
                    &group_thousands(blocked),
                    &format!("{ratio:.1}"),
                ],
            ),
            None => s.top_chattered_none.to_string(),
        },
    }
}

pub(crate) fn diff_stats(prev: &RenderedStats, next: &RenderedStats) -> StatsChanges {
    StatsChanges {
        status: prev.enabled != next.enabled
            || prev.active != next.active
            || prev.paused != next.paused
            || prev.badge != next.badge,
        total: prev.total != next.total,
        blocked: prev.blocked != next.blocked,
        ratio: prev.ratio != next.ratio,
        last: prev.last != next.last,
        top: prev.top != next.top,
        note: prev.note != next.note,
    }
}
