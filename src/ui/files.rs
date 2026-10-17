//! File menu: export the history as CSV, export and import settings.
//!
//! The files are small and written on the UI thread, like the settings
//! file itself.

use std::path::PathBuf;

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::HWND,
        UI::{
            Controls::Dialogs::{
                GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
                OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
            },
            WindowsAndMessaging::{MB_ICONINFORMATION, MB_ICONWARNING, MB_OK},
        },
    },
};

use super::{
    dialog::msg_box, history, keys_page, settings_form, state::current_lang, text::to_wide,
};
use crate::{
    app::state::SHARED,
    config,
    i18n::{fill, strings},
};

/// `kcfix-history-20261017-153045.csv`
fn stamped_name(prefix: &str, ext: &str) -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{prefix}-{:04}{:02}{:02}-{:02}{:02}{:02}.{ext}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// Open or save dialog for one file type. `default_name` pre-fills the
/// save dialog.
unsafe fn file_dialog(
    hwnd: HWND,
    save: bool,
    filter_name: &str,
    ext: &str,
    default_name: &str,
) -> Option<PathBuf> {
    let mut file = [0u16; 1024];
    let initial: Vec<u16> = default_name.encode_utf16().collect();
    let n = initial.len().min(file.len() - 1);
    file[..n].copy_from_slice(&initial[..n]);
    // "Name\0*.ext\0\0"
    let filter: Vec<u16> = format!("{filter_name}\0*.{ext}\0")
        .encode_utf16()
        .chain([0])
        .collect();
    let def_ext = to_wide(ext);
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrDefExt: PCWSTR(def_ext.as_ptr()),
        Flags: OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    let picked = if save {
        GetSaveFileNameW(&mut ofn).as_bool()
    } else {
        GetOpenFileNameW(&mut ofn).as_bool()
    };
    if !picked {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

unsafe fn report_write(hwnd: HWND, path: &std::path::Path, ok: bool) {
    let s = strings(current_lang());
    if ok {
        let text = fill(s.export_done_fmt, &[&path.display().to_string()]);
        msg_box(hwnd, s.title, &text, MB_OK | MB_ICONINFORMATION);
    } else {
        msg_box(hwnd, s.title, s.export_failed, MB_OK | MB_ICONWARNING);
    }
}

/// File > Export history (CSV).
pub(crate) unsafe fn export_history(hwnd: HWND) {
    let s = strings(current_lang());
    let Some(csv) = history::csv_snapshot() else {
        return;
    };
    let name = stamped_name("kcfix-history", "csv");
    if let Some(path) = file_dialog(hwnd, true, s.csv_filter, "csv", &name) {
        report_write(hwnd, &path, std::fs::write(&path, csv).is_ok());
    }
}

/// File > Export settings: the settings file as it is after a flush.
pub(crate) unsafe fn export_settings(hwnd: HWND) {
    let s = strings(current_lang());
    let name = stamped_name("kcfix-settings", "ini");
    if let Some(path) = file_dialog(hwnd, true, s.ini_filter, "ini", &name) {
        let ok =
            settings_form::persist_live() && std::fs::copy(config::config_path(), &path).is_ok();
        report_write(hwnd, &path, ok);
    }
}

/// File > Import settings: apply another settings file, keeping this PC's
/// statistics.
pub(crate) unsafe fn import_settings(hwnd: HWND) {
    let s = strings(current_lang());
    let Some(path) = file_dialog(hwnd, false, s.ini_filter, "ini", "") else {
        return;
    };
    let Some(mut imported) = config::load_from(&path) else {
        msg_box(hwnd, s.title, s.import_invalid, MB_OK | MB_ICONWARNING);
        return;
    };
    if let Some(shared) = SHARED.get() {
        let g = shared.lock().unwrap();
        imported.total_presses = g.debouncer.total_presses;
        imported.blocked_presses = g.debouncer.blocked_presses;
        imported.per_key_presses = g.debouncer.per_key_presses;
        imported.per_key_blocked = g.debouncer.per_key_blocked;
    }
    // On disk first: the save below takes file-owned values (per-key
    // windows) from the file.
    if !config::save(&imported) {
        msg_box(hwnd, s.title, s.save_failed, MB_OK | MB_ICONWARNING);
        return;
    }
    settings_form::load_into_form(hwnd, &imported);
    settings_form::on_save(hwnd);
    settings_form::on_language_changed(hwnd);
    keys_page::invalidate();
    let s = strings(current_lang());
    msg_box(hwnd, s.title, s.import_done, MB_OK | MB_ICONINFORMATION);
}
