//! Settings and Advanced tabs: load into the form, track unsaved edits,
//! validate, persist, and apply.
//!
//! Protection on/off, the language, and the Keys tab (per-key windows,
//! mouse filter) apply immediately (they are not "settings to review");
//! everything else is applied by the Save button, and the status line says
//! when there are unsaved changes.

use std::sync::atomic::Ordering;

use windows::{
    Win32::{
        Foundation::HWND,
        UI::Controls::Dialogs::{
            GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
            OPENFILENAMEW,
        },
    },
    core::{PCWSTR, PWSTR},
};

use super::super::{
    ids::*,
    state::{SAVED, current_lang, set_lang},
    text::{apply_language, combo_sel, get_text, set_text, to_wide},
};
use super::fields::{mark_dirty, refresh_status};
use super::persist::{SOUND_GEN, persist_live};
use crate::{
    app::state::SHARED,
    config,
    i18n::{LanguagePref, strings, system_language},
    input::sound::{play_notification_sound, play_path_for_preview},
};

/// Validate the sound path on a worker thread (it touches the disk and may
/// sit on an unreachable network share) and apply it when done (G7).
pub(super) fn apply_sound_async(path: Option<String>) {
    let generation = SOUND_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let validated = config::resolve_sound_file(path.as_deref());
        if let Some(shared) = SHARED.get() {
            let mut g = shared.lock().unwrap();
            // Checked under the lock: an older, slower validation can never
            // overwrite the result of a newer save.
            if SOUND_GEN.load(Ordering::SeqCst) == generation {
                g.sound_file = validated;
            }
        }
    });
}

/// Language combo: switch the UI right away and remember the choice.
pub(crate) unsafe fn on_language_changed(hwnd: HWND) {
    unsafe {
        // Row 0 is "Automatic" (follow Windows); the rest are fixed languages.
        let pref = LanguagePref::from_combo_index(combo_sel(hwnd, ID_COMBO_LANG));
        if let Some(saved) = SAVED.get() {
            saved.lock().unwrap().language = pref;
        }
        let lang = pref.resolve(system_language());
        if lang == current_lang() {
            // Only the stored choice changed (e.g. Automatic -> the language it
            // already resolves to): remember it, nothing to retranslate.
            persist_live();
            return;
        }
        set_lang(lang);
        persist_live();
        apply_language(hwnd);
        refresh_status(hwnd);
    }
}

pub(crate) unsafe fn preview_sound(hwnd: HWND) {
    let path = get_text(hwnd, ID_EDIT_SOUND_FILE);
    if path.is_empty() {
        // Blank means "system sound": preview exactly that.
        play_notification_sound(None);
    } else {
        play_path_for_preview(std::path::Path::new(&path));
    }
}

pub(crate) unsafe fn browse_sound(hwnd: HWND) {
    unsafe {
        let s = strings(current_lang());
        let mut file = [0u16; 1024];
        let current = to_wide(&get_text(hwnd, ID_EDIT_SOUND_FILE));
        let n = current.len().min(file.len() - 1);
        file[..n].copy_from_slice(&current[..n]);
        // "Name\0*.wav\0\0"
        let filter: Vec<u16> = format!("{}\0*.wav\0", s.sound_filter)
            .encode_utf16()
            .chain([0])
            .collect();
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(file.as_mut_ptr()),
            nMaxFile: file.len() as u32,
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
            let picked = String::from_utf16_lossy(&file[..len]);
            set_text(hwnd, ID_EDIT_SOUND_FILE, &picked);
            mark_dirty(hwnd);
        }
    }
}
