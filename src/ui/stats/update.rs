use std::sync::atomic::Ordering;

use windows::Win32::{Foundation::HWND, Graphics::Gdi::InvalidateRect, UI::WindowsAndMessaging::*};

use super::super::{
    ids::*,
    layout,
    state::current_lang,
    text::set_text,
    tray,
    widgets::{set_protection_on, set_toggle},
};
use super::model::{StatsChanges, StatsSnapshot};
use super::render::{diff_stats, render_stats};
use super::tick::{LAST_RENDERED, StatsTickGuard};
use crate::{
    app::state::{HOOK_ACTIVE, SHARED},
    i18n::strings,
};

pub(crate) unsafe fn update_stats(hwnd: HWND) {
    unsafe {
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
        (snapshot.pause, snapshot.elevated_foreground) = super::super::foreground::status();
        snapshot.hook_restarted_at = super::super::watchdog::last_restart();
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
            super::super::window::apply_visibility(hwnd);
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
}
