//! Home-tab statistics and header status, refreshed on the 500 ms timer.
//!
//! The timer does not touch windows blindly: the debouncer state is
//! snapshotted under a short lock, rendered to plain strings with no lock
//! held, and only controls whose text actually changed are updated. An idle
//! tick therefore performs zero `SetWindowTextW` calls.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

use windows::Win32::{Foundation::HWND, Graphics::Gdi::InvalidateRect, UI::WindowsAndMessaging::*};

use super::foreground::Pause;
use super::{
    ids::*,
    keys::vk_name,
    layout,
    state::current_lang,
    text::set_text,
    tray,
    widgets::{set_protection_on, set_toggle},
};
use crate::{
    app::state::{HOOK_ACTIVE, SHARED},
    i18n::{fill, strings, Strings},
};

/// Point-in-time copy of everything the readout needs, taken under one
/// short lock so no Win32 call ever runs while holding `SHARED`.
pub(crate) struct StatsSnapshot {
    pub cleaning_mode: bool,
    /// The user's protection switch.
    pub enabled: bool,
    /// Whether the keyboard hook is installed (false only after a failed
    /// install, G1).
    pub hook_active: bool,
    pub total_presses: u64,
    pub blocked_presses: u64,
    pub last_blocked: Option<(u8, Duration)>,
    pub top_chattered: Option<(u8, u64, f64)>,
    /// Paused for an app in front (`foreground`).
    pub pause: Option<Pause>,
    /// A window running as administrator is in front (we are not).
    pub elevated_foreground: bool,
    /// Local time the watchdog last restarted the hook.
    pub hook_restarted_at: Option<String>,
}

/// Fully rendered readout for one refresh, built with no lock held and no
/// window touched.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedStats {
    pub cleaning: bool,
    /// The switch position, mirrored into the toggle (and saved from it).
    pub enabled: bool,
    /// Whether input is actually filtered: switch on AND hook installed.
    /// Drives the badge and tray; never fed back into the switch, or a
    /// failed hook would get `bFilterEnabled=0` saved by the next Save.
    pub active: bool,
    pub paused: bool,
    pub badge: String,
    /// Home note under the tip: why protection is paused, an elevated
    /// window in front, or a hook restart.
    pub note: String,
    pub total: String,
    pub blocked: String,
    pub ratio: String,
    pub last: String,
    pub top: String,
}

/// Which parts of the window must be refreshed between two frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatsChanges {
    pub status: bool,
    pub total: bool,
    pub blocked: bool,
    pub ratio: bool,
    pub last: bool,
    pub top: bool,
    pub note: bool,
}

impl StatsChanges {
    const ALL: StatsChanges = StatsChanges {
        status: true,
        total: true,
        blocked: true,
        ratio: true,
        last: true,
        top: true,
        note: true,
    };
}

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

static LAST_RENDERED: Mutex<Option<RenderedStats>> = Mutex::new(None);

/// Forget the last frame so the next refresh repaints everything
/// (language change).
pub(crate) fn invalidate() {
    *LAST_RENDERED.lock().unwrap() = None;
}

/// Reentrancy guard for the 500 ms stats tick (ISSUE-003). Modal dialogs pump
/// nested messages, so `WM_TIMER` can re-enter `update_stats`; the nested tick
/// is dropped instead of piling up `SetWindowTextW` calls. On drop, ticks
/// slower than the budget are logged to aid hang diagnosis.
static STATS_TICK_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Ticks slower than this are reported; the timer fires every 500 ms.
const SLOW_TICK_MS: u128 = 250;

struct StatsTickGuard {
    started: std::time::Instant,
}
impl StatsTickGuard {
    fn enter() -> Option<StatsTickGuard> {
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
        }
    }
}

pub(crate) unsafe fn update_stats(hwnd: HWND) {
    let Some(_tick) = StatsTickGuard::enter() else {
        tracing::debug!("stats tick re-entered; dropping nested tick");
        return;
    };
    // 1. Snapshot under one short lock; the guard is dropped before any
    // Win32 call, so the hook thread can never wait on a blocked UI call.
    let snapshot = SHARED.get().map(|shared| {
        let g = shared.lock().unwrap();
        StatsSnapshot {
            cleaning_mode: g.debouncer.cleaning_mode,
            enabled: g.enabled,
            hook_active: HOOK_ACTIVE.load(Ordering::Relaxed),
            total_presses: g.debouncer.total_presses,
            blocked_presses: g.debouncer.blocked_presses,
            last_blocked: g.debouncer.last_blocked,
            top_chattered: g.debouncer.top_chattered_key(),
            pause: None,
            elevated_foreground: false,
            hook_restarted_at: None,
        }
    });
    let Some(mut snapshot) = snapshot else {
        return;
    };
    (snapshot.pause, snapshot.elevated_foreground) = super::foreground::status();
    snapshot.hook_restarted_at = super::watchdog::last_restart();
    let rendered = render_stats(strings(current_lang()), &snapshot);

    // 2. Diff against the last frame; the first frame updates everything.
    let changed = {
        let mut last = LAST_RENDERED.lock().unwrap();
        let changed = match &*last {
            Some(prev) => diff_stats(prev, &rendered),
            None => StatsChanges::ALL,
        };
        *last = Some(rendered.clone());
        changed
    };

    // 3. Touch only what actually changed. Cleaning compares against what
    // the window shows (not the last frame), so the first frame after
    // startup does not rebuild the page for nothing.
    if rendered.cleaning != layout::showing_cleaning() {
        layout::set_showing_cleaning(rendered.cleaning);
        super::window::apply_visibility(hwnd);
    }
    if changed.status {
        set_protection_on(rendered.active);
        set_toggle(hwnd, ID_TOGGLE_ENABLE, rendered.enabled);
        set_text(hwnd, ID_PILL, &rendered.badge);
        if let Ok(pill) = GetDlgItem(Some(hwnd), ID_PILL) {
            let _ = InvalidateRect(Some(pill), None, false);
        }
        tray::set_state(
            hwnd,
            if rendered.paused {
                tray::TrayState::Paused
            } else if rendered.active {
                tray::TrayState::On
            } else {
                tray::TrayState::Off
            },
        );
    }
    if changed.note {
        set_text(hwnd, ID_HOME_NOTE, &rendered.note);
    }
    if changed.total {
        set_text(hwnd, ID_STAT_TOTAL_VAL, &rendered.total);
    }
    if changed.blocked {
        set_text(hwnd, ID_STAT_BLOCKED_VAL, &rendered.blocked);
    }
    if changed.ratio {
        set_text(hwnd, ID_STAT_RATIO_VAL, &rendered.ratio);
    }
    if changed.last {
        set_text(hwnd, ID_STAT_LAST, &rendered.last);
    }
    if changed.top {
        set_text(hwnd, ID_STAT_TOP, &rendered.top);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn idle_snapshot() -> StatsSnapshot {
        StatsSnapshot {
            cleaning_mode: false,
            enabled: true,
            hook_active: true,
            total_presses: 0,
            blocked_presses: 0,
            last_blocked: None,
            top_chattered: None,
            pause: None,
            elevated_foreground: false,
            hook_restarted_at: None,
        }
    }

    #[test]
    fn paused_for_an_app_shows_paused_badge_and_why() {
        let snap = StatsSnapshot {
            pause: Some(Pause::App("game.exe".into())),
            ..idle_snapshot()
        };
        let r = render_stats(strings(Language::En), &snap);
        assert!(r.paused && !r.active && r.enabled);
        assert_eq!(r.badge, "◐ Paused");
        assert!(r.note.contains("game.exe"), "{}", r.note);
        let full = StatsSnapshot {
            pause: Some(Pause::Fullscreen("video.exe".into())),
            ..idle_snapshot()
        };
        assert!(render_stats(strings(Language::Ko), &full)
            .note
            .contains("전체 화면 앱(video.exe)"));
        // Switched off by the user: not "paused".
        let off = StatsSnapshot {
            enabled: false,
            ..snap
        };
        assert_eq!(
            render_stats(strings(Language::En), &off).badge,
            "○ Protection off"
        );
        assert!(diff_stats(&render_stats(strings(Language::En), &idle_snapshot()), &r).note);
    }

    #[test]
    fn notes_cover_elevated_windows_and_hook_restarts() {
        let s = strings(Language::En);
        let elevated = StatsSnapshot {
            elevated_foreground: true,
            ..idle_snapshot()
        };
        assert_eq!(render_stats(s, &elevated).note, s.note_elevated);
        // Not protecting anyway: no point warning.
        let off = StatsSnapshot {
            enabled: false,
            elevated_foreground: true,
            ..idle_snapshot()
        };
        assert_eq!(render_stats(s, &off).note, "");
        let restarted = StatsSnapshot {
            hook_restarted_at: Some("14:05".into()),
            ..idle_snapshot()
        };
        assert!(render_stats(s, &restarted).note.contains("14:05"));
        assert_eq!(render_stats(s, &idle_snapshot()).note, "");
    }

    #[test]
    fn render_idle_shows_placeholders() {
        let r = render_stats(strings(Language::Ko), &idle_snapshot());
        assert!(!r.cleaning);
        assert_eq!(r.badge, "● 보호 켜짐");
        assert_eq!(r.total, "0");
        assert_eq!(r.blocked, "0");
        assert_eq!(r.ratio, "0.0%");
        assert_eq!(r.last, "마지막으로 막은 키: 아직 없음");
        assert_eq!(r.top, "가장 자주 튄 키: 아직 없음");
    }

    #[test]
    fn render_blocked_formats_last_and_top() {
        let snap = StatsSnapshot {
            total_presses: 200,
            blocked_presses: 50,
            last_blocked: Some((0x20, Duration::from_millis(32))),
            top_chattered: Some((0x20, 40, 80.0)),
            ..idle_snapshot()
        };
        let r = render_stats(strings(Language::En), &snap);
        assert_eq!(r.ratio, "25.0%");
        assert_eq!(r.last, "Last blocked: Space (repeated after 32 ms)");
        assert_eq!(
            r.top,
            "Most affected key: Space (40 blocked, 80.0% of its presses)"
        );
        let ko = render_stats(strings(Language::Ko), &snap);
        assert_eq!(ko.last, "마지막으로 막은 키: Space (32ms 만에 다시 눌림)");
        assert_eq!(
            ko.top,
            "가장 자주 튄 키: Space (40회 막음, 이 키 입력의 80.0%)"
        );
    }

    #[test]
    fn render_groups_large_counters() {
        let snap = StatsSnapshot {
            total_presses: 1_234_567,
            blocked_presses: 1_000,
            ..idle_snapshot()
        };
        let r = render_stats(strings(Language::En), &snap);
        assert_eq!(r.total, "1,234,567");
        assert_eq!(r.blocked, "1,000");
        assert_eq!(r.ratio, "0.1%");
    }

    #[test]
    fn group_thousands_boundaries() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(100_000), "100,000");
        assert_eq!(group_thousands(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn render_cleaning_mode_and_paused_badge() {
        let snap = StatsSnapshot {
            cleaning_mode: true,
            enabled: false,
            ..idle_snapshot()
        };
        let r = render_stats(strings(Language::En), &snap);
        assert!(r.cleaning);
        assert!(!r.enabled);
        assert_eq!(r.badge, "○ Protection off");
    }

    #[test]
    fn failed_hook_shows_off_but_keeps_the_switch_on() {
        // G1 follow-up: the badge must not claim protection without a hook,
        // but the switch (which Save persists) must keep the user's choice.
        let snap = StatsSnapshot {
            hook_active: false,
            ..idle_snapshot()
        };
        let r = render_stats(strings(Language::En), &snap);
        assert!(r.enabled);
        assert!(!r.active);
        assert_eq!(r.badge, "○ Protection off");
        let healthy = render_stats(strings(Language::En), &idle_snapshot());
        assert!(diff_stats(&healthy, &r).status);
    }

    #[test]
    fn stats_tick_guard_drops_nested_entry() {
        let first = StatsTickGuard::enter();
        assert!(first.is_some());
        // A nested timer tick (e.g. from a modal dialog pump) is dropped.
        assert!(StatsTickGuard::enter().is_none());
        drop(first);
        assert!(StatsTickGuard::enter().is_some());
    }

    #[test]
    fn diff_identical_frames_update_nothing() {
        let r = render_stats(strings(Language::Ko), &idle_snapshot());
        assert_eq!(diff_stats(&r, &r), StatsChanges::default());
    }

    #[test]
    fn diff_flags_only_the_changed_control() {
        let prev = render_stats(strings(Language::Ko), &idle_snapshot());
        let snap = StatsSnapshot {
            total_presses: 1,
            ..idle_snapshot()
        };
        let next = render_stats(strings(Language::Ko), &snap);
        // Only the total counter changed (ratio stays "0.0%").
        assert_eq!(
            diff_stats(&prev, &next),
            StatsChanges {
                total: true,
                ..StatsChanges::default()
            }
        );
    }

    #[test]
    fn cleaning_toggle_changes_no_text() {
        // Entering cleaning mode swaps the page (compared against what the
        // window shows), but must not rewrite any statistics text.
        let prev = render_stats(strings(Language::Ko), &idle_snapshot());
        let snap = StatsSnapshot {
            cleaning_mode: true,
            ..idle_snapshot()
        };
        let next = render_stats(strings(Language::Ko), &snap);
        assert!(next.cleaning && !prev.cleaning);
        assert_eq!(diff_stats(&prev, &next), StatsChanges::default());
    }

    #[test]
    fn diff_flags_status_when_protection_flips() {
        let prev = render_stats(strings(Language::En), &idle_snapshot());
        let snap = StatsSnapshot {
            enabled: false,
            ..idle_snapshot()
        };
        let next = render_stats(strings(Language::En), &snap);
        let changed = diff_stats(&prev, &next);
        assert!(changed.status);
        assert!(!changed.total && !changed.ratio);
    }

    #[test]
    fn render_and_diff_sustained_tick_load() {
        // The 500 ms timer path must stay cheap: 10k render+diff cycles must
        // finish far inside one tick budget (ISSUE-003 regression guard).
        let rendered = render_stats(strings(Language::Ko), &idle_snapshot());
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            let next = render_stats(strings(Language::Ko), &idle_snapshot());
            assert_eq!(diff_stats(&rendered, &next), StatsChanges::default());
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "render+diff too slow: {:?}",
            started.elapsed()
        );
    }
}
