use std::sync::{Mutex, atomic::Ordering};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

use super::super::{
    commands, elevation, foreground, history, hotkeys,
    ids::*,
    keys_page,
    layout::{Page, STAT_DIVIDERS, TAB_TRACK, cards, current_page, sc, showing_cleaning},
    menu,
    paint::{
        ThemeColors, apply_control_themes, apply_window_chrome, cached_brush, draw_rounded_card,
        scaled_rect, static_colors, theme_is_dark,
    },
    settings_form::{self, on_save},
    state::{SAVED, set_lang},
    text::apply_language,
    tray::{add_tray, remove_tray},
    widgets::flip_toggle,
};
use crate::config::Settings;

use super::create::{apply_visibility, create_controls, switch_page};
use super::nonce::mint_update_exit_nonce;
use super::proc::TASKBAR_CREATED;

pub(super) unsafe fn on_create(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    unsafe {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        let init = &*(cs.lpCreateParams as *const Settings);
        set_lang(init.ui_language());
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
        let _ = SAVED.set(Mutex::new(init.clone()));
        mint_update_exit_nonce();

        apply_window_chrome(hwnd);
        create_controls(hwnd, cs.hInstance);
        history::init(hwnd);
        keys_page::init(hwnd);
        apply_control_themes(hwnd);
        // Fills combos and the menu, then the form loads the saved values.
        apply_language(hwnd);
        settings_form::load_into_form(hwnd, init);
        apply_visibility(hwnd);
        add_tray(hwnd);
        foreground::configure(
            &init.auto_disable_apps,
            &init.app_profiles,
            init.auto_disable_fullscreen,
        );
        for problem in hotkeys::apply(hwnd, &init.hotkey_toggle, &init.hotkey_cleaning) {
            tracing::warn!("{problem}");
        }
        // Menus show the registered hotkeys.
        menu::install(hwnd);
        elevation::init();
        // Like a dialog: no dotted focus box on the tab until the keyboard is
        // used (Windows clears this itself on the first Tab/arrow press).
        let _ = SendMessageW(
            hwnd,
            WM_CHANGEUISTATE,
            Some(WPARAM(
                (UIS_SET as usize) | ((UISF_HIDEFOCUS as usize) << 16),
            )),
            Some(LPARAM(0)),
        );
        let _ = SetTimer(Some(hwnd), TIMER_STATS, 500, None);
        LRESULT(0)
    }
}

pub(super) unsafe fn on_paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (width, height) = (rc.right - rc.left, rc.bottom - rc.top);
        let colors = ThemeColors::new(theme_is_dark());

        // Double buffering.
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
        let old_bmp = SelectObject(mem_dc, mem_bmp.into());

        FillRect(mem_dc, &rc, cached_brush(colors.bg));
        let cleaning = showing_cleaning();
        let page = current_page();
        if !cleaning {
            draw_rounded_card(
                mem_dc,
                &scaled_rect(TAB_TRACK),
                colors.tab_track,
                colors.tab_track,
                sc(10),
            );
        }
        for &card in cards(page, cleaning) {
            draw_rounded_card(
                mem_dc,
                &scaled_rect(card),
                colors.card_bg,
                colors.card_border,
                sc(12),
            );
        }
        if page == Page::Home && !cleaning {
            let pen = CreatePen(PS_SOLID, 1, colors.card_border);
            let old_pen = SelectObject(mem_dc, pen.into());
            for (x, y0, y1) in STAT_DIVIDERS {
                let _ = MoveToEx(mem_dc, sc(x), sc(y0), None);
                let _ = LineTo(mem_dc, sc(x), sc(y1));
            }
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(pen.into());
        }

        let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(mem_bmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
}

pub(super) unsafe fn on_ctl_color_static(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let hdc = HDC(wparam.0 as *mut _);
        let id = GetWindowLongW(HWND(lparam.0 as *mut _), GWL_ID);
        let colors = ThemeColors::new(theme_is_dark());
        let (fg, bg) = match super::super::layout::spec(id) {
            Some(spec) => static_colors(spec.kind, spec.on_card, &colors),
            None => (colors.text_primary, colors.card_bg),
        };
        let _ = SetTextColor(hdc, fg);
        let _ = SetBkColor(hdc, bg);
        let _ = SetBkMode(hdc, TRANSPARENT);
        LRESULT(cached_brush(bg).0 as isize)
    }
}

pub(super) unsafe fn on_command(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    unsafe {
        let id = (wparam.0 & 0xFFFF) as i32;
        let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
        let clicked = code == BN_CLICKED || code == BN_DOUBLECLICKED;

        // Enter inside a field (IsDialogMessage sends IDOK): save on the
        // settings pages, otherwise ignore. IDCANCEL (Esc) is ignored.
        if id == IDOK.0 {
            match current_page() {
                _ if showing_cleaning() => {}
                Page::Settings | Page::Advanced => on_save(hwnd),
                Page::Keys => keys_page::on_enter(hwnd),
                _ => {}
            }
            return;
        }
        if id == IDCANCEL.0 {
            return;
        }
        // Menu bar / accelerator commands carry no control handle.
        if lparam.0 == 0 {
            commands::run(hwnd, id as usize);
            return;
        }

        if let Some(page) = Page::from_tab_id(id) {
            if clicked {
                switch_page(hwnd, page);
            }
            return;
        }
        if let Some(spec) = super::super::layout::spec(id)
            && spec.kind.is_toggle()
            && clicked
        {
            let on = flip_toggle(hwnd, id);
            if id == ID_TOGGLE_ENABLE {
                commands::set_protection(hwnd, on);
            } else if id == ID_TOGGLE_MOUSE {
                keys_page::on_mouse_toggled(hwnd, on);
            } else if id == ID_TOGGLE_WHEEL {
                keys_page::on_wheel_toggled(hwnd, on);
            } else {
                settings_form::mark_dirty(hwnd);
            }
            return;
        }
        match (id, code) {
            (ID_PILL, BN_CLICKED | BN_DOUBLECLICKED) => commands::run(hwnd, MENU_TOGGLE),
            (ID_BTN_CLEAN_MODE, BN_CLICKED) => commands::run(hwnd, MENU_CLEANING),
            (ID_BTN_CLEAN_END, BN_CLICKED) => commands::set_cleaning(hwnd, false),
            (ID_BTN_RESET_STATS, BN_CLICKED) => commands::run(hwnd, MENU_RESET_STATS),
            (ID_BTN_OPEN_CPL, BN_CLICKED) => commands::run(hwnd, MENU_OPEN_CPL),
            (ID_BTN_CLEAR_LOG, BN_CLICKED) => commands::run(hwnd, MENU_CLEAR_LOG),
            (ID_BTN_SAVE, BN_CLICKED) => on_save(hwnd),
            (ID_BTN_SOUND_TEST, BN_CLICKED) => settings_form::preview_sound(hwnd),
            (ID_BTN_SOUND_BROWSE, BN_CLICKED) => settings_form::browse_sound(hwnd),
            (ID_HIST_SEARCH, EN_CHANGE) => history::on_search_changed(hwnd),
            (
                ID_EDIT_THRESHOLD
                | ID_EDIT_SOUND_FILE
                | ID_EDIT_MIN_CHATTER
                | ID_EDIT_LOG_LIMIT
                | ID_EDIT_DEFER
                | ID_EDIT_AUTO_APPS
                | ID_EDIT_APP_PROFILES
                | ID_EDIT_HOTKEY_TOGGLE
                | ID_EDIT_HOTKEY_CLEAN,
                EN_CHANGE,
            ) => settings_form::mark_dirty(hwnd),
            (ID_COMBO_MEASURE, CBN_SELCHANGE) => settings_form::mark_dirty(hwnd),
            (ID_COMBO_LANG, CBN_SELCHANGE) => settings_form::on_language_changed(hwnd),
            (ID_HIST_VIEW, CBN_SELCHANGE) => history::on_view_changed(hwnd),
            (ID_KEYS_COMBO, CBN_SELCHANGE) => keys_page::on_combo_changed(hwnd),
            (ID_BTN_KEY_SET, BN_CLICKED) => keys_page::apply_override(hwnd),
            (ID_BTN_KEY_RESET, BN_CLICKED) => keys_page::clear_override(hwnd),
            (ID_BTN_KEY_STATS, BN_CLICKED) => keys_page::clear_key_stats(hwnd),
            (ID_BTN_KEY_APPLY_ALL, BN_CLICKED) => keys_page::apply_all_suggestions(hwnd),
            (ID_BTN_MOUSE_MS_SET, BN_CLICKED) => keys_page::apply_mouse_threshold(hwnd),
            _ => {}
        }
    }
}

pub(super) unsafe fn on_destroy(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_STATS);
        let _ = hotkeys::apply(hwnd, "", "");
        settings_form::persist_live();
        remove_tray(hwnd);
        // The hook thread is stopped by the bootstrap after the message loop.
        PostQuitMessage(0);
    }
}
