//! User commands shared by the menu bar, the tray menu, and buttons.
//!
//! Every entry point resolves to a `MENU_*` id and lands in [`run`], so a
//! feature reachable from three places has exactly one implementation.

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

use super::{
    dialog::{msg_box, show_about},
    history,
    ids::*,
    settings_form::persist_live,
    state::current_lang,
    stats::update_stats,
    text::to_wide,
    tray,
    update::spawn_update_check,
};
use crate::{app::state::SHARED, config, i18n::strings};

/// Bring the window back from the tray (or from minimized).
pub(crate) unsafe fn show_window(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_SHOW);
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    let _ = SetForegroundWindow(hwnd);
}

pub(crate) unsafe fn hide_to_tray(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_HIDE);
    tray::notify_hidden_once(hwnd);
}

/// Turn protection on/off immediately; the tick syncs pill, toggle, tray.
pub(crate) unsafe fn set_protection(hwnd: HWND, on: bool) {
    if let Some(g) = SHARED.get() {
        g.lock().unwrap().enabled = on;
    }
    update_stats(hwnd);
}

fn protection_on() -> bool {
    SHARED.get().is_none_or(|g| g.lock().unwrap().enabled)
}

pub(crate) unsafe fn set_cleaning(hwnd: HWND, on: bool) {
    if let Some(g) = SHARED.get() {
        g.lock().unwrap().debouncer.cleaning_mode = on;
    }
    if on {
        show_window(hwnd);
    }
    update_stats(hwnd);
}

/// Open a folder or URL with the shell on a worker thread: `ShellExecuteW`
/// can take seconds (cold browser or Explorer start), which must not freeze
/// the window (G7). The shell requires COM on the calling thread.
fn shell_open(target: String) {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    std::thread::spawn(move || unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let t = to_wide(&target);
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(t.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if com.is_ok() {
            CoUninitialize();
        }
    });
}

pub(crate) unsafe fn run(hwnd: HWND, cmd: usize) {
    let s = strings(current_lang());
    match cmd {
        MENU_SHOW => show_window(hwnd),
        MENU_HIDE => hide_to_tray(hwnd),
        MENU_TOGGLE => set_protection(hwnd, !protection_on()),
        MENU_CLEANING => set_cleaning(hwnd, true),
        MENU_RESET_STATS => {
            let answer = msg_box(
                hwnd,
                s.reset_stats_btn,
                s.reset_confirm,
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            );
            if answer == IDYES {
                if let Some(g) = SHARED.get() {
                    g.lock().unwrap().debouncer.reset_stats();
                }
                persist_live();
                update_stats(hwnd);
                history::refresh(hwnd, true);
            }
        }
        MENU_CLEAR_LOG => {
            if let Some(g) = SHARED.get() {
                g.lock().unwrap().debouncer.clear_log();
            }
            history::refresh(hwnd, true);
        }
        MENU_OPEN_CPL => {
            let _ = std::process::Command::new("control.exe")
                .arg("keyboard")
                .spawn();
        }
        MENU_OPEN_CONFIG => {
            if let Some(dir) = config::config_path().parent() {
                let _ = std::fs::create_dir_all(dir);
                shell_open(dir.to_string_lossy().into_owned());
            }
        }
        MENU_UPDATE => spawn_update_check(hwnd),
        MENU_HOMEPAGE => shell_open(config::HOMEPAGE_URL.to_string()),
        MENU_ABOUT => show_about(hwnd),
        MENU_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}
