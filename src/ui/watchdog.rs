//! Hook watchdog: Windows removes a low-level hook without notice when its
//! callback is too slow (`LowLevelHooksTimeout`), after which the window
//! would keep showing "protection on" while nothing is filtered.
//!
//! Every 10 s the watchdog looks at the hook's event counter. When the hook
//! saw nothing although the user was active (`GetLastInputInfo`), it sends
//! one marked probe event (a release of an unused key) that a live hook
//! swallows and counts. If the probe does not arrive within a second, the
//! hook thread is replaced. Probing only while the user is active means the
//! probe never keeps an idle PC awake.
//!
//! (Raw keyboard input would be the obvious detector, but registering for
//! it stops the hook from seeing input while this window is in front.)

use std::sync::{Mutex, atomic::Ordering};

use windows::Win32::{
    System::SystemInformation::GetTickCount,
    UI::Input::KeyboardAndMouse::{
        GetLastInputInfo, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        LASTINPUTINFO, SendInput, VIRTUAL_KEY,
    },
};

use crate::input::hook::{HOOK_EVENTS, INJECT_MARK, PROBE_VK, PROBES_SEEN, restart};

/// Timer ticks (500 ms) between checks: 10 s.
const CHECK_TICKS: u32 = 20;
/// Ticks to wait for a probe: 1 s.
const PROBE_TICKS: u32 = 2;

/// What the watchdog does at a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The hook saw events, or nobody used the PC: nothing to do.
    Fine,
    /// The user was active but the hook saw nothing: send a probe.
    Probe,
}

/// Decision at a check: `hooked` events since the last check, whether
/// there was user input in that period, and whether a window running as
/// administrator is in front while we are not. Such a window's input never
/// reaches our hook, and UIPI silently drops a probe aimed at it (with no
/// error from `SendInput`): probing then would restart a healthy hook
/// every check (2026-10-22 audit ISSUE-004).
pub(crate) fn check(hooked: u64, user_active: bool, elevated_foreground: bool) -> Step {
    if hooked == 0 && user_active && !elevated_foreground {
        Step::Probe
    } else {
        Step::Fine
    }
}

/// Whether an outstanding probe proves the hook dead: not seen after
/// `waited` ticks.
pub(crate) fn probe_failed(seen: bool, waited: u32) -> bool {
    !seen && waited >= PROBE_TICKS
}

struct State {
    ticks: u32,
    hooked: u64,
    last_check_tick: u32,
    /// (ticks waited, probe count when sent) while a probe is out.
    probe: Option<(u32, u64)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    ticks: 0,
    hooked: 0,
    last_check_tick: 0,
    probe: None,
});
/// Local time (`HH:MM`) of the last restart, for the Home note.
static LAST_RESTART: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn last_restart() -> Option<String> {
    LAST_RESTART.lock().unwrap().clone()
}

fn clock_hh_mm() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// Whether there was user input since `since` (a `GetTickCount` value).
unsafe fn input_since(since: u32) -> bool {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        GetLastInputInfo(&mut info).as_bool() && info.dwTime.wrapping_sub(since) as i32 > 0
    }
}

/// Send one watchdog probe. Returns whether it left this process: a probe
/// that never left cannot come back, so the caller must not arm a wait for
/// it (ISSUE-001).
unsafe fn send_probe() -> bool {
    unsafe {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(PROBE_VK),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: INJECT_MARK,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32) != 0
    }
}

/// Timer tick (UI thread). Returns whether the hook was restarted.
pub(crate) unsafe fn tick() -> bool {
    unsafe {
        let mut st = STATE.lock().unwrap();
        if let Some((waited, sent_at)) = st.probe {
            let seen = PROBES_SEEN.load(Ordering::Relaxed) > sent_at;
            let waited = waited + 1;
            if seen {
                st.probe = None;
            } else if probe_failed(seen, waited) {
                st.probe = None;
                st.hooked = HOOK_EVENTS.load(Ordering::Relaxed);
                drop(st);
                tracing::warn!("keyboard hook missed its probe; restarting it");
                super::diag::log_line("hook probe missed; restarting hook");
                let ok = restart();
                if ok {
                    *LAST_RESTART.lock().unwrap() = Some(clock_hh_mm());
                }
                return ok;
            } else {
                st.probe = Some((waited, sent_at));
            }
            return false;
        }
        st.ticks += 1;
        if st.ticks < CHECK_TICKS {
            return false;
        }
        st.ticks = 0;
        let hooked_now = HOOK_EVENTS.load(Ordering::Relaxed);
        let hooked = hooked_now - st.hooked;
        st.hooked = hooked_now;
        let since = st.last_check_tick;
        st.last_check_tick = GetTickCount();
        // The first check has no reference time: just take one.
        if since == 0 {
            return false;
        }
        let elevated_foreground = super::foreground::status().1;
        if check(hooked, input_since(since), elevated_foreground) == Step::Probe {
            let sent_at = PROBES_SEEN.load(Ordering::Relaxed);
            drop(st);
            // Arm the wait only when the probe actually left: otherwise the next
            // check retries instead of restarting a healthy hook (ISSUE-001).
            if send_probe() {
                STATE.lock().unwrap().probe = Some((0, sent_at));
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_only_when_the_user_was_active_and_the_hook_silent() {
        assert_eq!(check(0, true, false), Step::Probe);
        assert_eq!(check(3, true, false), Step::Fine);
        // Idle PC: never send input (it would reset the idle timer).
        assert_eq!(check(0, false, false), Step::Fine);
    }

    #[test]
    fn no_probe_while_an_administrator_window_is_in_front() {
        // Its input bypasses our hook and the probe cannot reach it: the
        // silence proves nothing about the hook.
        assert_eq!(check(0, true, true), Step::Fine);
    }

    #[test]
    fn a_missing_probe_fails_after_a_second() {
        assert!(!probe_failed(false, PROBE_TICKS - 1));
        assert!(probe_failed(false, PROBE_TICKS));
        assert!(!probe_failed(true, PROBE_TICKS + 5));
    }
}
