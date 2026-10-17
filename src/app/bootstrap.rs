//! Startup orchestration: settings → shared state → hook → message loop.
//!
//! `run` owns the single-instance guard for the whole process lifetime, so
//! dropping it (process exit) is the only way the mutex is released.

use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Mutex},
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::HWND, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};

use super::{
    instance::{bring_existing_to_front, show_already_running, try_acquire_single_instance},
    state::{SHARED, START, START_CLOCK_MS},
};
use crate::{config, debounce::Debouncer, input};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

/// Build the debouncer from persisted settings and publish shared state.
fn init_shared(settings: &config::Settings) {
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
    debouncer.per_key_presses = settings.per_key_presses;
    debouncer.per_key_blocked = settings.per_key_blocked;
    debouncer.record_all_keys = settings.record_all_keys;
    debouncer.set_log_limit(settings.log_limit);

    let _ = SHARED.set(Mutex::new(super::state::Shared {
        debouncer,
        enabled: settings.enabled,
        play_sound: settings.play_sound,
        sound_file: config::resolve_sound_file(settings.sound_file.as_deref()),
        exclude_injected: settings.exclude_injected,
    }));
    let _ = START.set(Instant::now());
    let _ = START_CLOCK_MS.set(local_time_of_day_ms());
}

fn local_time_of_day_ms() -> u64 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    ((t.wHour as u64 * 60 + t.wMinute as u64) * 60 + t.wSecond as u64) * 1000
        + t.wMilliseconds as u64
}

/// Update-helper handshake: the helper relaunches this executable with
/// `KCFIX_UPDATE_READY_FILE` set and waits until the new version proves
/// it is alive by writing its pid there. The hook and window are set up
/// below; acknowledge shortly after so the helper only accepts a process
/// that actually started. The message loop keeps us alive afterwards,
/// which is what the helper's stability wait requires.
fn acknowledge_update_handshake() {
    if let Some(path) = std::env::var_os("KCFIX_UPDATE_READY_FILE").map(PathBuf::from) {
        std::env::remove_var("KCFIX_UPDATE_READY_FILE");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = std::fs::write(&path, std::process::id().to_string());
        });
    }
}

pub(crate) fn run(settings: config::Settings) {
    // Subscriber for slow-tick and update-path diagnostics (ISSUE-001/003).
    // Best effort: a second init (tests) or no console (GUI) must not fail startup.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
    init_shared(&settings);

    #[cfg(windows)]
    let _instance_guard = match try_acquire_single_instance() {
        Some(guard) => guard,
        None => {
            bring_existing_to_front();
            show_already_running(&settings.language);
            return;
        }
    };

    acknowledge_update_handshake();
    // Staging leftovers of earlier updates (G4); disk I/O off the UI thread.
    std::thread::spawn(crate::updater::cleanup_stale_staging);

    unsafe {
        // High-precision multimedia timer (1ms resolution)
        timeBeginPeriod(1);

        use windows::Win32::Foundation::HINSTANCE;
        let hmodule = GetModuleHandleW(None).expect("own module handle");
        let hinstance = HINSTANCE(hmodule.0);
        // The hook pumps its own thread (ISSUE-002); this thread only runs
        // UI. Installed before the window so the first frame shows the
        // real protection state.
        let hook = input::hook::spawn_hook_thread();
        super::state::HOOK_ACTIVE.store(hook.is_some(), Ordering::Relaxed);
        let hwnd: HWND = crate::ui::create_window(hinstance, &settings);
        if hwnd.is_invalid() {
            if let Some(hook) = hook {
                hook.stop();
            }
            timeEndPeriod(1);
            return;
        }
        crate::ui::show_initial(hwnd, settings.minimize_on_launch);
        if hook.is_none() {
            // Never let the UI pretend to protect without a hook (G1).
            crate::ui::show_hook_failed(hwnd);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Dialog-style keyboard handling: Tab/Shift+Tab move between
            // controls, Space toggles, Enter in a field saves (IDOK).
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // The window already flushed settings in WM_DESTROY; unhook last
        // so keystrokes stay filtered until the very end.
        if let Some(hook) = hook {
            hook.stop();
        }
        timeEndPeriod(1);
    }
}
