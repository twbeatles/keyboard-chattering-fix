//! KeyboardChatteringFix: lightweight keyboard anti-chatter filter.
//!
//! A single native executable. A low-level keyboard hook drops a repeated
//! press of the same key when it arrives inside the debounce threshold
//! after its release. Auto-repeat while holding a key always passes.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod debounce;
mod i18n;
mod ui;
mod updater;

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicIsize, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{Diagnostics::Debug::MessageBeep, LibraryLoader::GetModuleHandleW},
    UI::WindowsAndMessaging::*,
};

use debounce::{Debouncer, KeyEventKind};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
    fn PlaySoundW(pszSound: *const u16, hmod: *mut core::ffi::c_void, fdwSound: u32) -> i32;
}

const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_FILENAME: u32 = 0x00020000;

/// Play the validated custom sound, falling back to the system beep.
/// `sound_file` must already be validated by `config::resolve_sound_file`:
/// the hook calls this on every blocked keystroke, so no filesystem I/O
/// may happen here (ISSUE-003).
pub fn play_notification_sound(sound_file: Option<&std::path::Path>) {
    use std::os::windows::ffi::OsStrExt;
    unsafe {
        if let Some(path) = sound_file {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            if PlaySoundW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                SND_ASYNC | SND_FILENAME | SND_NODEFAULT,
            ) != 0
            {
                return;
            }
        }
        let _ = MessageBeep(MB_ICONEXCLAMATION);
    }
}

pub struct Shared {
    pub debouncer: Debouncer,
    pub enabled: bool,
    pub play_sound: bool,
    /// Custom sound validated once at startup/save time, never per keystroke.
    pub sound_file: Option<PathBuf>,
    pub exclude_injected: bool,
}

/// Guard for the process-wide single-instance mutex (ISSUE-001).
#[cfg(windows)]
struct InstanceGuard {
    handle: Option<windows::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

/// Claim the single-instance mutex. Returns `None` when another instance
/// already holds it. CLI one-shots (`--version`, `--check-update`) run
/// before this and are exempt on purpose.
#[cfg(windows)]
fn try_acquire_single_instance() -> Option<InstanceGuard> {
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        },
    };
    unsafe {
        let name = HSTRING::from("Local\\KeyboardChatteringFix_SingleInstance");
        match CreateMutexW(None, false, &name) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                    None
                } else {
                    Some(InstanceGuard {
                        handle: Some(handle),
                    })
                }
            }
            // Fail open: a broken mutex must never brick the filter.
            Err(_) => Some(InstanceGuard { handle: None }),
        }
    }
}

use windows::Win32::UI::WindowsAndMessaging::LLKHF_INJECTED;

pub static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
pub static START: OnceLock<Instant> = OnceLock::new();
pub static HOOK: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode >= 0 {
        let kind = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyEventKind::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(KeyEventKind::Up),
            _ => None,
        };
        if let Some(kind) = kind {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if let (Some(shared), Some(start)) = (SHARED.get(), START.get()) {
                let mut g = shared.lock().unwrap();

                // 1. Cleaning mode: drops everything, or exits on Enter (0x0D)
                if g.debouncer.cleaning_mode {
                    let _ = g
                        .debouncer
                        .decide(kb.vkCode as usize, kind, start.elapsed());
                    return LRESULT(1);
                }

                // 2. Never touch synthesized input if exclude_injected is enabled
                if g.exclude_injected && (kb.flags & LLKHF_INJECTED).0 != 0 {
                    return CallNextHookEx(None, ncode, wparam, lparam);
                }

                // 3. Debounce decision
                if g.enabled
                    && !g
                        .debouncer
                        .decide(kb.vkCode as usize, kind, start.elapsed())
                {
                    if g.play_sound && kind == KeyEventKind::Down {
                        play_notification_sound(g.sound_file.as_deref());
                    }
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// Attach to the parent console so `--version` / `--check-update` output is
/// visible when launched from a terminal. The GUI build sets
/// `windows_subsystem = "windows"`, which otherwise detaches stdio.
#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::AttachConsole;
    unsafe {
        let _ = AttachConsole(0xFFFFFFFF);
    }
}

/// Bring the already-running instance's window to the front (best effort).
#[cfg(windows)]
fn bring_existing_to_front() {
    use windows::{core::w, Win32::UI::WindowsAndMessaging::*};
    unsafe {
        // Must match the class registered in `ui::create_window`.
        if let Ok(hwnd) = FindWindowW(w!("KeyboardChatteringFixMain"), None) {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Tell the user a previous instance is already filtering input.
#[cfg(windows)]
fn show_already_running(language: &crate::i18n::Language) {
    use windows::{
        core::HSTRING,
        Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK},
    };
    let s = crate::i18n::strings(*language);
    let text = HSTRING::from(s.already_running);
    let title = HSTRING::from(s.title);
    unsafe {
        let _ = MessageBoxW(None, &text, &title, MB_OK | MB_ICONINFORMATION);
    }
}

/// Lightweight CLI used by the release smoke check and by users in a
/// terminal. Returns true when the process was fully handled and the GUI
/// must not start.
fn handle_cli() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        #[cfg(windows)]
        attach_parent_console();
        println!("keyboard-chattering-fix {}", config::VERSION);
        return true;
    }
    if args.iter().any(|a| a == "--check-update") {
        #[cfg(windows)]
        attach_parent_console();
        match updater::check_for_update() {
            Ok(manifest) => println!("update available {}", manifest.version),
            Err(updater::UpdateError::NoUpdate) => println!("up to date"),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return true;
    }
    false
}

fn main() {
    if handle_cli() {
        return;
    }
    let settings = config::load();
    let mut debouncer = Debouncer::new(settings.threshold_ms);
    for (vk, ms) in settings.per_key_ms.iter().enumerate() {
        if let Some(ms) = ms {
            debouncer.set_key_threshold(vk as u8, Some(*ms));
        }
    }
    debouncer.set_measure_from(settings.measure_from);
    debouncer.set_min_chatter_ms(settings.min_chatter_ms);
    debouncer.total_presses = settings.total_presses;
    debouncer.blocked_presses = settings.blocked_presses;
    debouncer.record_all_keys = settings.record_all_keys;
    debouncer.log_limit = settings.log_limit;

    #[cfg(windows)]
    let _instance_guard = match try_acquire_single_instance() {
        Some(guard) => guard,
        None => {
            bring_existing_to_front();
            show_already_running(&settings.language);
            return;
        }
    };

    let _ = SHARED.set(Mutex::new(Shared {
        debouncer,
        enabled: settings.enabled,
        play_sound: settings.play_sound,
        sound_file: config::resolve_sound_file(settings.sound_file.as_deref()),
        exclude_injected: settings.exclude_injected,
    }));
    let _ = START.set(Instant::now());

    // Update-helper handshake: the helper relaunches this executable with
    // `KCFIX_UPDATE_READY_FILE` set and waits until the new version proves
    // it is alive by writing its pid there. The hook and window are set up
    // below; acknowledge shortly after so the helper only accepts a process
    // that actually started. The message loop keeps us alive afterwards,
    // which is what the helper's stability wait requires.
    if let Some(path) = std::env::var_os("KCFIX_UPDATE_READY_FILE").map(PathBuf::from) {
        std::env::remove_var("KCFIX_UPDATE_READY_FILE");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = std::fs::write(&path, std::process::id().to_string());
        });
    }

    unsafe {
        // High-precision multimedia timer (1ms resolution)
        timeBeginPeriod(1);

        use windows::Win32::Foundation::HINSTANCE;
        let hmodule = GetModuleHandleW(None).expect("own module handle");
        let hinstance = HINSTANCE(hmodule.0);
        let hwnd: HWND = ui::create_window(hinstance, &settings);
        if hwnd.is_invalid() {
            timeEndPeriod(1);
            return;
        }
        let hook =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinstance, 0).unwrap_or_default();
        HOOK.store(hook.0 as isize, Ordering::Relaxed);
        ui::show_initial(hwnd, settings.minimize_on_launch);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        timeEndPeriod(1);
    }
}

#[cfg(test)]
mod tests {
    /// Second acquisition fails while the first guard is alive, so a double
    /// launch can never install two hooks (ISSUE-001). NOTE: fails if a real
    /// app instance holds the mutex — run on CI, not next to a live tray app.
    #[test]
    #[cfg(windows)]
    fn second_acquisition_fails_while_first_held() {
        let _first = super::try_acquire_single_instance().expect("first acquisition");
        assert!(super::try_acquire_single_instance().is_none());
    }
}
