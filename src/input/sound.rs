//! Audio feedback for blocked keystrokes.
//!
//! The path is validated and pre-encoded once at startup/save time by
//! `config::resolve_sound_file`; the hook plays the cached buffer with zero
//! allocation and no filesystem I/O per blocked keystroke (ISSUE-003).

use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_ICONEXCLAMATION;

const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_FILENAME: u32 = 0x00020000;

/// Play the pre-encoded custom sound, falling back to the system beep.
/// Takes the cached buffer so the keyboard-hook hot path never allocates.
pub(crate) fn play_notification_sound(sound_wide: Option<&[u16]>) {
    unsafe {
        if let Some(wide) = sound_wide {
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

/// Cold-path preview for the sound-test button: encodes on the fly.
/// The hook never uses this; it plays the pre-encoded buffer instead.
pub(crate) fn play_path_for_preview(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    play_notification_sound(Some(&wide));
}

#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(pszSound: *const u16, hmod: *mut core::ffi::c_void, fdwSound: u32) -> i32;
}
