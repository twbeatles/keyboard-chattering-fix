use std::time::Duration;

use super::super::foreground::Pause;
use super::model::{StatsChanges, StatsSnapshot};
use super::render::{diff_stats, group_thousands, render_stats};
use super::tick::StatsTickGuard;
use crate::i18n::{Language, strings};

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
    assert!(
        render_stats(strings(Language::Ko), &full)
            .note
            .contains("전체 화면 앱(video.exe)")
    );
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
    // finish far inside one tick budget (ISSUE-003 regression guard,
    // PERF-002: the old 10 s bound could not catch a real regression).
    let rendered = render_stats(strings(Language::Ko), &idle_snapshot());
    let started = std::time::Instant::now();
    for _ in 0..10_000 {
        let next = render_stats(strings(Language::Ko), &idle_snapshot());
        assert_eq!(diff_stats(&rendered, &next), StatsChanges::default());
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "render+diff too slow: {:?}",
        started.elapsed()
    );
}
