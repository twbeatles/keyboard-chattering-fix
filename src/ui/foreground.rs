//! Foreground check on the 500 ms timer: pause protection while a listed or
//! full-screen app is in front (games, anti-cheat), and notice windows that
//! run as administrator, whose input a non-elevated hook cannot see.
//!
//! The process name and elevation are looked up only when the foreground
//! window changes; the hook reads a single `suspended` flag.

use std::sync::{Mutex, OnceLock};

use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
            PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
            GetWindowThreadProcessId,
        },
    },
};

use crate::app::state::SHARED;

/// Normalized app names from the settings text (`game.exe; Other.EXE`):
/// lowercase, without `.exe`. Separators: `;` `,` `/` and line breaks.
pub(crate) fn parse_app_list(text: &str) -> Vec<String> {
    text.split([';', ',', '/', '\n', '\r'])
        .map(normalize)
        .filter(|s| !s.is_empty())
        .collect()
}

fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower
        .strip_suffix(".exe")
        .unwrap_or(&lower)
        .trim()
        .to_string()
}

/// Whether the executable file name (`Game.exe`) is on the list.
pub(crate) fn app_listed(list: &[String], exe_file: &str) -> bool {
    let name = normalize(exe_file);
    !name.is_empty() && list.contains(&name)
}

/// A window covering its whole monitor is full screen.
pub(crate) fn covers_monitor(win: RECT, monitor: RECT) -> bool {
    win.left <= monitor.left
        && win.top <= monitor.top
        && win.right >= monitor.right
        && win.bottom >= monitor.bottom
}

/// Why protection is paused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pause {
    /// A listed app (its file name).
    App(String),
    /// A full-screen app (its file name).
    Fullscreen(String),
}

pub(crate) fn pause_reason(
    listed: bool,
    fullscreen: bool,
    pause_fullscreen: bool,
    exe_file: &str,
) -> Option<Pause> {
    if listed {
        Some(Pause::App(exe_file.to_string()))
    } else if fullscreen && pause_fullscreen {
        Some(Pause::Fullscreen(exe_file.to_string()))
    } else {
        None
    }
}

// --- Window glue (UI thread) ---------------------------------------------

struct Config {
    apps: Vec<String>,
    fullscreen: bool,
}

struct Cache {
    hwnd: isize,
    exe: String,
    elevated: bool,
}

static CONFIG: Mutex<Config> = Mutex::new(Config {
    apps: Vec::new(),
    fullscreen: false,
});
static CACHE: Mutex<Cache> = Mutex::new(Cache {
    hwnd: 0,
    exe: String::new(),
    elevated: false,
});
/// (pause reason, elevated window in front while we are not elevated).
static STATUS: Mutex<(Option<Pause>, bool)> = Mutex::new((None, false));

/// Apply the saved choices (startup and every save).
pub(crate) fn configure(apps: &str, fullscreen: bool) {
    let mut c = CONFIG.lock().unwrap();
    c.apps = parse_app_list(apps);
    c.fullscreen = fullscreen;
    // Re-evaluate the current window on the next tick.
    CACHE.lock().unwrap().hwnd = 0;
}

/// Current pause reason and whether an elevated window is in front.
pub(crate) fn status() -> (Option<Pause>, bool) {
    STATUS.lock().unwrap().clone()
}

/// Whether this process runs elevated (checked once).
pub(crate) fn self_elevated() -> bool {
    static ELEVATED: OnceLock<bool> = OnceLock::new();
    *ELEVATED.get_or_init(|| unsafe { token_elevated(GetCurrentProcess()).unwrap_or(false) })
}

unsafe fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    let ok = GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut elevation as *mut _ as *mut _),
        std::mem::size_of::<TOKEN_ELEVATION>() as u32,
        &mut len,
    )
    .is_ok();
    let _ = CloseHandle(token);
    ok.then_some(elevation.TokenIsElevated != 0)
}

/// File name and elevation of a process. A token that cannot be read
/// belongs to a process above our integrity level: treat it as elevated.
unsafe fn describe_process(pid: u32) -> (String, bool) {
    let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return (String::new(), false);
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let exe = if QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        PWSTR(buf.as_mut_ptr()),
        &mut len,
    )
    .is_ok()
    {
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or("").to_string()
    } else {
        String::new()
    };
    let elevated = token_elevated(process).unwrap_or(true);
    let _ = CloseHandle(process);
    (exe, elevated)
}

/// Desktop and shell windows are never "full-screen apps".
unsafe fn is_shell(hwnd: HWND) -> bool {
    if hwnd == GetDesktopWindow() || hwnd == GetShellWindow() {
        return true;
    }
    let mut class = [0u16; 32];
    let n = GetClassNameW(hwnd, &mut class) as usize;
    let class = String::from_utf16_lossy(&class[..n]);
    matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
}

unsafe fn is_fullscreen(hwnd: HWND) -> bool {
    if is_shell(hwnd) {
        return false;
    }
    let mut win = RECT::default();
    if GetWindowRect(hwnd, &mut win).is_err() {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    GetMonitorInfoW(monitor, &mut info).as_bool() && covers_monitor(win, info.rcMonitor)
}

/// Timer tick. `own` is the main window (never paused for itself).
/// Returns whether the status changed.
pub(crate) unsafe fn tick(own: HWND) -> bool {
    let fg = GetForegroundWindow();
    let (apps, pause_fullscreen) = {
        let c = CONFIG.lock().unwrap();
        (c.apps.clone(), c.fullscreen)
    };
    let next = if fg.is_invalid() || fg == own {
        (None, false)
    } else {
        let (exe, elevated) = {
            let mut cache = CACHE.lock().unwrap();
            if cache.hwnd != fg.0 as isize {
                let mut pid = 0u32;
                GetWindowThreadProcessId(fg, Some(&mut pid));
                let (exe, elevated) = describe_process(pid);
                *cache = Cache {
                    hwnd: fg.0 as isize,
                    exe,
                    elevated,
                };
            }
            (cache.exe.clone(), cache.elevated)
        };
        let listed = app_listed(&apps, &exe);
        let fullscreen = pause_fullscreen && !listed && is_fullscreen(fg);
        let reason = pause_reason(listed, fullscreen, pause_fullscreen, &exe);
        (reason, elevated && !self_elevated())
    };
    let changed = {
        let mut st = STATUS.lock().unwrap();
        let changed = *st != next;
        *st = next.clone();
        changed
    };
    if changed {
        if let Some(shared) = SHARED.get() {
            shared.lock().unwrap().suspended = next.0.is_some();
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_list_accepts_common_separators_and_spellings() {
        let list = parse_app_list(" Game.exe; obs64 ,  /other.EXE\n\nmy game.exe ");
        assert_eq!(list, ["game", "obs64", "other", "my game"]);
        assert!(app_listed(&list, "GAME.EXE"));
        assert!(app_listed(&list, "my game.exe"));
        assert!(!app_listed(&list, "gamer.exe"));
        assert!(!app_listed(&list, ""));
        assert!(parse_app_list(" ; , ").is_empty());
    }

    #[test]
    fn full_screen_means_covering_the_whole_monitor() {
        let mon = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        assert!(covers_monitor(mon, mon));
        // Borderless windows often overhang by a few pixels.
        let over = RECT {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(covers_monitor(over, mon));
        let maximized_with_taskbar = RECT {
            bottom: 1040,
            ..mon
        };
        assert!(!covers_monitor(maximized_with_taskbar, mon));
    }

    #[test]
    fn listed_apps_win_and_full_screen_needs_the_option() {
        assert_eq!(
            pause_reason(true, true, true, "game.exe"),
            Some(Pause::App("game.exe".into()))
        );
        assert_eq!(
            pause_reason(false, true, true, "video.exe"),
            Some(Pause::Fullscreen("video.exe".into()))
        );
        assert_eq!(pause_reason(false, true, false, "video.exe"), None);
        assert_eq!(pause_reason(false, false, true, "x.exe"), None);
    }
}
