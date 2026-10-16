//! Telemetry readout: virtual-key names and the periodic stats refresh.
//!
//! The 500 ms timer does not touch windows blindly: the debouncer state is
//! snapshotted under a short lock, rendered to plain strings with no lock
//! held, and only controls whose text actually changed are updated. An idle
//! tick therefore performs zero `SetWindowTextW` calls (previously seven,
//! every tick, while holding the shared lock).

use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

use super::{ids::*, state::current_lang, text::set_text};
use crate::{
    app::state::SHARED,
    i18n::{strings, Strings},
};

/// Point-in-time copy of everything the telemetry readout needs, taken
/// under one short lock so no Win32 call ever runs while holding `SHARED`.
pub(crate) struct StatsSnapshot {
    pub cleaning_mode: bool,
    pub enabled: bool,
    pub total_presses: u64,
    pub blocked_presses: u64,
    pub last_blocked: Option<(u8, Duration)>,
    pub top_chattered: Option<(u8, u64, f64)>,
}

/// Fully rendered telemetry strings for one refresh, built with no lock
/// held and no window touched. `banner: None` means "hide the banner".
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderedStats {
    pub banner: Option<String>,
    pub badge: String,
    pub total: String,
    pub blocked: String,
    pub ratio: String,
    pub last: String,
    pub top: String,
}

/// Pure rendering: debouncer snapshot plus strings in, control texts out.
/// Unit-testable without windows, hooks, or locks.
pub(crate) fn render_stats(s: &Strings, snap: &StatsSnapshot) -> RenderedStats {
    let ratio = if snap.total_presses == 0 {
        0.0
    } else {
        snap.blocked_presses as f64 * 100.0 / snap.total_presses as f64
    };
    RenderedStats {
        banner: snap.cleaning_mode.then(|| s.clean_mode_banner.to_string()),
        badge: (if snap.enabled {
            s.status_active
        } else {
            s.status_paused
        })
        .to_string(),
        total: snap.total_presses.to_string(),
        blocked: snap.blocked_presses.to_string(),
        ratio: format!("{ratio:.1}%"),
        last: match snap.last_blocked {
            Some((vk, gap)) => s.last_blocked_fmt.replacen("{}", &vk_name(vk), 1).replacen(
                "{}",
                &gap.as_millis().to_string(),
                1,
            ),
            None => s.last_blocked_none.to_string(),
        },
        top: match snap.top_chattered {
            Some((vk, blocked, ratio)) => s
                .top_chattered_fmt
                .replacen("{}", &vk_name(vk), 1)
                .replacen("{}", &blocked.to_string(), 1)
                .replacen("{:.1}", &format!("{ratio:.1}"), 1),
            None => s.top_chattered_none.to_string(),
        },
    }
}

/// Per-control "changed" flags in [`RenderedStats`] field order
/// (banner, badge, total, blocked, ratio, last, top). The caller only
/// touches windows whose flag is set.
pub(crate) fn diff_stats(prev: &RenderedStats, next: &RenderedStats) -> [bool; 7] {
    [
        prev.banner != next.banner,
        prev.badge != next.badge,
        prev.total != next.total,
        prev.blocked != next.blocked,
        prev.ratio != next.ratio,
        prev.last != next.last,
        prev.top != next.top,
    ]
}

fn vk_name(vk: u8) -> String {
    match vk {
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x10 => "Shift".to_string(),
        0x11 => "Ctrl".to_string(),
        0x12 => "Alt".to_string(),
        0x13 => "Pause".to_string(),
        0x14 => "CapsLock".to_string(),
        0x1B => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x23 => "End".to_string(),
        0x24 => "Home".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x2C => "PrintScreen".to_string(),
        0x2D => "Insert".to_string(),
        0x2E => "Delete".to_string(),
        0x30..=0x39 | 0x41..=0x5A => (vk as char).to_string(),
        0x5B => "LWin".to_string(),
        0x5C => "RWin".to_string(),
        0x5D => "Apps".to_string(),
        0x5F => "Sleep".to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x6A => "Num*".to_string(),
        0x6B => "Num+".to_string(),
        0x6D => "Num-".to_string(),
        0x6E => "Num.".to_string(),
        0x6F => "Num/".to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x90 => "NumLock".to_string(),
        0x91 => "ScrollLock".to_string(),
        0xA0 => "LShift".to_string(),
        0xA1 => "RShift".to_string(),
        0xA2 => "LCtrl".to_string(),
        0xA3 => "RCtrl".to_string(),
        0xA4 => "LAlt".to_string(),
        0xA5 => "RAlt".to_string(),
        0xA6 => "BrowserBack".to_string(),
        0xA7 => "BrowserForward".to_string(),
        0xA8 => "BrowserRefresh".to_string(),
        0xA9 => "BrowserStop".to_string(),
        0xAA => "BrowserSearch".to_string(),
        0xAB => "BrowserFavorites".to_string(),
        0xAC => "BrowserHome".to_string(),
        0xAD => "VolumeMute".to_string(),
        0xAE => "VolumeDown".to_string(),
        0xAF => "VolumeUp".to_string(),
        0xB0 => "MediaNext".to_string(),
        0xB1 => "MediaPrev".to_string(),
        0xB2 => "MediaStop".to_string(),
        0xB3 => "MediaPlayPause".to_string(),
        0xB4 => "LaunchMail".to_string(),
        0xB5 => "LaunchMedia".to_string(),
        0xB6 => "LaunchApp1".to_string(),
        0xB7 => "LaunchApp2".to_string(),
        0xBA => ";".to_string(),
        0xBB => "=".to_string(),
        0xBC => ",".to_string(),
        0xBD => "-".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xC0 => "`".to_string(),
        0xDB => "[".to_string(),
        0xDC => "\\".to_string(),
        0xDD => "]".to_string(),
        0xDE => "'".to_string(),
        _ => format!("0x{vk:02X}"),
    }
}

pub(crate) unsafe fn update_stats(hwnd: HWND) {
    // 1. Snapshot under one short lock; the guard is dropped before any
    // Win32 call, so the hook thread can never wait on a blocked UI call.
    let snapshot = SHARED.get().map(|shared| {
        let g = shared.lock().unwrap();
        StatsSnapshot {
            cleaning_mode: g.debouncer.cleaning_mode,
            enabled: g.enabled,
            total_presses: g.debouncer.total_presses,
            blocked_presses: g.debouncer.blocked_presses,
            last_blocked: g.debouncer.last_blocked,
            top_chattered: g.debouncer.top_chattered_key(),
        }
    });
    let Some(snapshot) = snapshot else {
        return;
    };
    let rendered = render_stats(&strings(current_lang()), &snapshot);

    // 2. Diff against the last frame; the first frame updates everything.
    static LAST_RENDERED: OnceLock<Mutex<Option<RenderedStats>>> = OnceLock::new();
    let changed = {
        let mut last = LAST_RENDERED
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap();
        let changed = match &*last {
            Some(prev) => diff_stats(prev, &rendered),
            None => [true; 7],
        };
        *last = Some(rendered.clone());
        changed
    };

    // 3. Touch only the controls whose text actually changed.
    if changed[0] {
        if let Ok(banner) = GetDlgItem(Some(hwnd), ID_CLEAN_BANNER) {
            match &rendered.banner {
                Some(text) => {
                    let _ = ShowWindow(banner, SW_SHOW);
                    set_text(hwnd, ID_CLEAN_BANNER, text);
                }
                None => {
                    let _ = ShowWindow(banner, SW_HIDE);
                }
            }
        }
    }
    if changed[1] {
        set_text(hwnd, ID_HEADER_BADGE, &rendered.badge);
    }
    if changed[2] {
        set_text(hwnd, ID_STAT_TOTAL_VAL, &rendered.total);
    }
    if changed[3] {
        set_text(hwnd, ID_STAT_CHATTER_VAL, &rendered.blocked);
    }
    if changed[4] {
        set_text(hwnd, ID_STAT_RATIO_VAL, &rendered.ratio);
    }
    if changed[5] {
        set_text(hwnd, ID_STATIC_LAST, &rendered.last);
    }
    if changed[6] {
        set_text(hwnd, ID_STATIC_TOP_CHATTER, &rendered.top);
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
            total_presses: 0,
            blocked_presses: 0,
            last_blocked: None,
            top_chattered: None,
        }
    }

    #[test]
    fn render_idle_shows_placeholders() {
        let r = render_stats(&strings(Language::Ko), &idle_snapshot());
        assert_eq!(r.banner, None);
        assert_eq!(r.badge, "● 보호 동작 중");
        assert_eq!(r.total, "0");
        assert_eq!(r.blocked, "0");
        assert_eq!(r.ratio, "0.0%");
        assert_eq!(r.last, "최근 차단: 이력 없음");
        assert_eq!(r.top, "최다 채터링 키: 아직 없음");
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
        let r = render_stats(&strings(Language::En), &snap);
        assert_eq!(r.banner, None);
        assert_eq!(r.ratio, "25.0%");
        assert_eq!(r.last, "Last blocked: Space (32ms gap)");
        assert_eq!(r.top, "Top Chatter Key: Space (40 blocks, 80.0%)");
    }

    #[test]
    fn render_cleaning_mode_shows_banner_and_paused_badge() {
        let snap = StatsSnapshot {
            cleaning_mode: true,
            enabled: false,
            ..idle_snapshot()
        };
        let r = render_stats(&strings(Language::En), &snap);
        assert!(r.banner.unwrap().contains("Cleaning Mode"));
        assert_eq!(r.badge, "○ Protection Paused");
    }

    #[test]
    fn diff_identical_frames_update_nothing() {
        let r = render_stats(&strings(Language::Ko), &idle_snapshot());
        assert_eq!(diff_stats(&r, &r), [false; 7]);
    }

    #[test]
    fn diff_flags_only_the_changed_control() {
        let prev = render_stats(&strings(Language::Ko), &idle_snapshot());
        let snap = StatsSnapshot {
            total_presses: 1,
            ..idle_snapshot()
        };
        let next = render_stats(&strings(Language::Ko), &snap);
        // Only the total counter changed (ratio stays "0.0%").
        assert_eq!(
            diff_stats(&prev, &next),
            [false, false, true, false, false, false, false]
        );
    }

    #[test]
    fn diff_detects_banner_toggle() {
        let prev = render_stats(&strings(Language::Ko), &idle_snapshot());
        let snap = StatsSnapshot {
            cleaning_mode: true,
            ..idle_snapshot()
        };
        let next = render_stats(&strings(Language::Ko), &snap);
        let changed = diff_stats(&prev, &next);
        assert!(changed[0]);
        assert_eq!(&changed[1..], &[false; 6]);
    }
}
