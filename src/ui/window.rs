//! Main window: message dispatch (`wndproc`), control construction, painting,
//! and teardown.
//!
//! The procedure only routes messages; each domain lives in its own module:
//! `controls` builds children, `paint` draws, `text`/`stats` refresh content,
//! `settings_form` saves, `tray` owns the notification icon, and `update`
//! drives the release-update flow.

use std::sync::{atomic::Ordering, Mutex};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::SystemServices::{SS_CENTER, SS_LEFT},
        UI::WindowsAndMessaging::*,
    },
};

use super::{
    controls::child,
    ids::*,
    paint::{apply_window_chrome, draw_rounded_card, get_fonts, theme_is_dark, ThemeColors},
    settings_form::on_save,
    state::{LANG, SAVED},
    stats::update_stats,
    text::{apply_language, get_check, set_check, to_wide},
    tray::{add_tray, remove_tray, show_tray_menu},
};
use crate::{
    app::state::{HOOK, SHARED},
    config::{self, Settings},
    debounce::MeasureFrom,
    i18n::{strings, Language},
    input::sound::play_path_for_preview,
};

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            let init = &*(cs.lpCreateParams as *const Settings);
            let hinstance = cs.hInstance;
            LANG.store(
                if init.language == Language::En { 1 } else { 0 },
                Ordering::Relaxed,
            );

            apply_window_chrome(hwnd);

            let fonts = get_fonts();

            // 1. Cleaning Banner (y: 6..34, hidden by default)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_CENTER.0),
                16,
                6,
                432,
                28,
                ID_CLEAN_BANNER,
                hinstance,
                fonts.body,
            );
            if let Ok(b) = GetDlgItem(Some(hwnd), ID_CLEAN_BANNER) {
                let _ = ShowWindow(b, SW_HIDE);
            }

            // 2. Header Card (y: 38..92)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                46,
                260,
                24,
                ID_HEADER_TITLE,
                hinstance,
                fonts.title,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                70,
                260,
                18,
                ID_HEADER_SUB,
                hinstance,
                fonts.subtitle,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_CENTER.0),
                300,
                52,
                134,
                24,
                ID_HEADER_BADGE,
                hinstance,
                fonts.badge,
            );

            // 3. Card 1: Telemetry (y: 102..232)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                112,
                400,
                20,
                ID_CARD1_TITLE,
                hinstance,
                fonts.section,
            );
            // Column 1
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                138,
                125,
                18,
                ID_STAT_TOTAL_LABEL,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                156,
                125,
                24,
                ID_STAT_TOTAL_VAL,
                hinstance,
                fonts.stat_val,
            );
            // Column 2
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                168,
                138,
                125,
                18,
                ID_STAT_CHATTER_LABEL,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                168,
                156,
                125,
                24,
                ID_STAT_CHATTER_VAL,
                hinstance,
                fonts.stat_val,
            );
            // Column 3
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                308,
                138,
                120,
                18,
                ID_STAT_RATIO_LABEL,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                308,
                156,
                120,
                24,
                ID_STAT_RATIO_VAL,
                hinstance,
                fonts.stat_val,
            );
            // Last blocked & top chatter chips
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                188,
                400,
                18,
                ID_STATIC_LAST,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                208,
                400,
                18,
                ID_STATIC_TOP_CHATTER,
                hinstance,
                fonts.body,
            );

            // 4. Card 2: Filter Settings (y: 242..432)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                252,
                400,
                20,
                ID_CARD2_TITLE,
                hinstance,
                fonts.section,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                276,
                100,
                20,
                ID_LABEL_THRESHOLD,
                hinstance,
                fonts.body,
            );
            let edit = child(
                hwnd,
                w!("EDIT"),
                "",
                WINDOW_STYLE(WS_BORDER.0 | WS_TABSTOP.0 | ES_NUMBER as u32),
                132,
                274,
                55,
                24,
                ID_EDIT_THRESHOLD,
                hinstance,
                fonts.body,
            );
            let t = to_wide(&init.threshold_ms.to_string());
            let _ = SetWindowTextW(edit, PCWSTR(t.as_ptr()));
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                195,
                276,
                230,
                20,
                ID_LABEL_UNIT,
                hinstance,
                fonts.body,
            );

            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                28,
                304,
                400,
                20,
                ID_CHECK_ENABLE,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                28,
                326,
                280,
                20,
                ID_CHECK_SOUND,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_PUSHBUTTON as u32 | WS_TABSTOP.0),
                320,
                323,
                110,
                24,
                ID_BTN_SOUND_TEST,
                hinstance,
                fonts.body,
            );

            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                350,
                250,
                18,
                ID_LABEL_SOUND_FILE,
                hinstance,
                fonts.body,
            );
            let edit_s = child(
                hwnd,
                w!("EDIT"),
                "",
                WINDOW_STYLE(WS_BORDER.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL as u32),
                28,
                370,
                404,
                24,
                ID_EDIT_SOUND_FILE,
                hinstance,
                fonts.body,
            );
            if let Some(ref path) = init.sound_file {
                let p = to_wide(path);
                let _ = SetWindowTextW(edit_s, PCWSTR(p.as_ptr()));
            }

            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                28,
                402,
                195,
                20,
                ID_CHECK_AUTOSTART,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                230,
                402,
                200,
                20,
                ID_CHECK_MINIMIZE,
                hinstance,
                fonts.body,
            );

            // 5. Card 3: Helpful Tools (y: 442..506)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                452,
                400,
                20,
                ID_CARD3_TITLE,
                hinstance,
                fonts.section,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_PUSHBUTTON as u32 | WS_TABSTOP.0),
                28,
                474,
                195,
                28,
                ID_BTN_CLEAN_MODE,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_PUSHBUTTON as u32 | WS_TABSTOP.0),
                235,
                474,
                195,
                28,
                ID_BTN_OPEN_CPL,
                hinstance,
                fonts.body,
            );

            // 6. Card 4: Advanced Preferences (y: 516..700)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                526,
                400,
                20,
                ID_CARD4_TITLE,
                hinstance,
                fonts.section,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                548,
                120,
                20,
                ID_LABEL_MEASURE,
                hinstance,
                fonts.body,
            );
            let combo_m = child(
                hwnd,
                w!("COMBOBOX"),
                "",
                WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | WS_TABSTOP.0),
                150,
                545,
                280,
                100,
                ID_COMBO_MEASURE,
                hinstance,
                fonts.body,
            );
            let opt1 = to_wide(strings(init.language).measure_release);
            let opt2 = to_wide(strings(init.language).measure_press);
            SendMessageW(
                combo_m,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(opt1.as_ptr() as isize)),
            );
            SendMessageW(
                combo_m,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(opt2.as_ptr() as isize)),
            );
            SendMessageW(
                combo_m,
                CB_SETCURSEL,
                Some(WPARAM(if init.measure_from == MeasureFrom::Press {
                    1
                } else {
                    0
                })),
                Some(LPARAM(0)),
            );

            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                574,
                120,
                20,
                ID_LABEL_LANG,
                hinstance,
                fonts.body,
            );
            let combo_l = child(
                hwnd,
                w!("COMBOBOX"),
                "",
                WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | WS_TABSTOP.0),
                150,
                571,
                140,
                100,
                ID_COMBO_LANG,
                hinstance,
                fonts.body,
            );
            for name in ["한국어", "English"] {
                let v = to_wide(name);
                SendMessageW(
                    combo_l,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(v.as_ptr() as isize)),
                );
            }
            SendMessageW(
                combo_l,
                CB_SETCURSEL,
                Some(WPARAM(init.language.combo_index())),
                Some(LPARAM(0)),
            );

            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                300,
                574,
                130,
                20,
                ID_CHECK_CLOSE_TO_TRAY,
                hinstance,
                fonts.body,
            );

            // Advanced numeric filters (y: 600..646)
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                600,
                120,
                20,
                ID_LABEL_MIN_CHATTER,
                hinstance,
                fonts.body,
            );
            let edit_min = child(
                hwnd,
                w!("EDIT"),
                "",
                WINDOW_STYLE(WS_BORDER.0 | WS_TABSTOP.0 | ES_NUMBER as u32),
                150,
                598,
                55,
                24,
                ID_EDIT_MIN_CHATTER,
                hinstance,
                fonts.body,
            );
            let t_min = to_wide(&init.min_chatter_ms.to_string());
            let _ = SetWindowTextW(edit_min, PCWSTR(t_min.as_ptr()));
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                212,
                600,
                218,
                20,
                ID_LABEL_MIN_UNIT,
                hinstance,
                fonts.body,
            );

            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                28,
                626,
                120,
                20,
                ID_LABEL_LOG_LIMIT,
                hinstance,
                fonts.body,
            );
            let edit_log = child(
                hwnd,
                w!("EDIT"),
                "",
                WINDOW_STYLE(WS_BORDER.0 | WS_TABSTOP.0 | ES_NUMBER as u32),
                150,
                624,
                55,
                24,
                ID_EDIT_LOG_LIMIT,
                hinstance,
                fonts.body,
            );
            let t_log = to_wide(&init.log_limit.to_string());
            let _ = SetWindowTextW(edit_log, PCWSTR(t_log.as_ptr()));
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_LEFT.0),
                212,
                626,
                218,
                20,
                ID_LABEL_LOG_UNIT,
                hinstance,
                fonts.body,
            );

            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                28,
                652,
                402,
                20,
                ID_CHECK_EXCLUDE_INJECTED,
                hinstance,
                fonts.body,
            );
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0),
                28,
                674,
                402,
                20,
                ID_CHECK_RECORD_ALL,
                hinstance,
                fonts.body,
            );

            // 7. Bottom Actions (y: 712..)
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_DEFPUSHBUTTON as u32 | WS_TABSTOP.0),
                16,
                712,
                432,
                38,
                ID_BTN_SAVE,
                hinstance,
                fonts.section,
            );
            child(
                hwnd,
                w!("STATIC"),
                "",
                WINDOW_STYLE(SS_CENTER.0),
                16,
                756,
                432,
                20,
                ID_STATIC_HINT,
                hinstance,
                fonts.subtitle,
            );

            let _ = SAVED.set(Mutex::new(init.clone()));
            set_check(hwnd, ID_CHECK_ENABLE, init.enabled);
            set_check(hwnd, ID_CHECK_SOUND, init.play_sound);
            set_check(hwnd, ID_CHECK_MINIMIZE, init.minimize_on_launch);
            set_check(hwnd, ID_CHECK_CLOSE_TO_TRAY, init.close_to_tray);
            set_check(hwnd, ID_CHECK_EXCLUDE_INJECTED, init.exclude_injected);
            set_check(hwnd, ID_CHECK_RECORD_ALL, init.record_all_keys);
            set_check(hwnd, ID_CHECK_AUTOSTART, config::autostart_enabled());

            apply_language(hwnd);
            add_tray(hwnd);
            let _ = SetTimer(Some(hwnd), 1, 500, None);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let width = rc.right - rc.left;
            let height = rc.bottom - rc.top;

            let is_dark = theme_is_dark();
            let colors = ThemeColors::new(is_dark);

            // Double buffering
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
            let old_bmp = SelectObject(mem_dc, mem_bmp.into());

            // Fill canvas
            let bg_brush = CreateSolidBrush(colors.bg);
            FillRect(mem_dc, &rc, bg_brush);
            let _ = DeleteObject(bg_brush.into());

            // Draw Card 0 (Header): 16, 38, 448, 92
            let c0 = RECT {
                left: 16,
                top: 38,
                right: 448,
                bottom: 92,
            };
            draw_rounded_card(mem_dc, &c0, colors.card_bg, colors.card_border, 10);

            // Draw Card 1 (Telemetry): 16, 102, 448, 232
            let c1 = RECT {
                left: 16,
                top: 102,
                right: 448,
                bottom: 232,
            };
            draw_rounded_card(mem_dc, &c1, colors.card_bg, colors.card_border, 10);

            // Telemetry grid divider lines
            let div_pen = CreatePen(PS_SOLID, 1, colors.card_border);
            let old_pen = SelectObject(mem_dc, div_pen.into());
            let _ = MoveToEx(mem_dc, 156, 138, None);
            let _ = LineTo(mem_dc, 156, 180);
            let _ = MoveToEx(mem_dc, 296, 138, None);
            let _ = LineTo(mem_dc, 296, 180);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(div_pen.into());

            // Draw Card 2 (Settings): 16, 242, 448, 432
            let c2 = RECT {
                left: 16,
                top: 242,
                right: 448,
                bottom: 432,
            };
            draw_rounded_card(mem_dc, &c2, colors.card_bg, colors.card_border, 10);

            // Draw Card 3 (Tools): 16, 442, 448, 506
            let c3 = RECT {
                left: 16,
                top: 442,
                right: 448,
                bottom: 506,
            };
            draw_rounded_card(mem_dc, &c3, colors.card_bg, colors.card_border, 10);

            // Draw Card 4 (Preferences): 16, 516, 448, 700
            let c4 = RECT {
                left: 16,
                top: 516,
                right: 448,
                bottom: 700,
            };
            draw_rounded_card(mem_dc, &c4, colors.card_bg, colors.card_border, 10);

            // Draw Status Pill in Header
            let enabled = SHARED
                .get()
                .map(|g| g.lock().unwrap().enabled)
                .unwrap_or(true);
            let (badge_bg, badge_border) = if enabled {
                (colors.status_active_bg, colors.status_active_fg)
            } else {
                (colors.status_paused_bg, colors.status_paused_fg)
            };
            let badge_rc = RECT {
                left: 296,
                top: 50,
                right: 438,
                bottom: 78,
            };
            draw_rounded_card(mem_dc, &badge_rc, badge_bg, badge_border, 14);

            let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
            SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(mem_bmp.into());
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let c_hwnd = HWND(lparam.0 as *mut _);
            let id = GetWindowLongW(c_hwnd, GWL_ID);
            let is_dark = theme_is_dark();
            let colors = ThemeColors::new(is_dark);
            let _ = SetBkMode(hdc, TRANSPARENT);

            match id {
                ID_CLEAN_BANNER => {
                    let _ = SetTextColor(hdc, colors.cleaning_banner_fg);
                    LRESULT(CreateSolidBrush(colors.cleaning_banner_bg).0 as isize)
                }
                ID_HEADER_TITLE => {
                    let _ = SetTextColor(hdc, colors.text_primary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_HEADER_SUB
                | ID_STAT_TOTAL_LABEL
                | ID_STAT_CHATTER_LABEL
                | ID_LABEL_UNIT
                | ID_LABEL_THRESHOLD
                | ID_LABEL_SOUND_FILE
                | ID_LABEL_MEASURE
                | ID_LABEL_LANG
                | ID_LABEL_MIN_CHATTER
                | ID_LABEL_MIN_UNIT
                | ID_LABEL_LOG_LIMIT
                | ID_LABEL_LOG_UNIT => {
                    let _ = SetTextColor(hdc, colors.text_secondary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_HEADER_BADGE => {
                    let enabled = SHARED
                        .get()
                        .map(|g| g.lock().unwrap().enabled)
                        .unwrap_or(true);
                    let (fg, bg) = if enabled {
                        (colors.status_active_fg, colors.status_active_bg)
                    } else {
                        (colors.status_paused_fg, colors.status_paused_bg)
                    };
                    let _ = SetTextColor(hdc, fg);
                    LRESULT(CreateSolidBrush(bg).0 as isize)
                }
                ID_CARD1_TITLE | ID_CARD2_TITLE | ID_CARD3_TITLE | ID_CARD4_TITLE => {
                    let _ = SetTextColor(hdc, colors.accent);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_STAT_TOTAL_VAL | ID_STAT_CHATTER_VAL => {
                    let _ = SetTextColor(hdc, colors.text_primary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_STAT_RATIO_LABEL => {
                    let _ = SetTextColor(hdc, colors.text_secondary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_STAT_RATIO_VAL => {
                    let _ = SetTextColor(hdc, colors.accent);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_STATIC_LAST | ID_STATIC_TOP_CHATTER => {
                    let _ = SetTextColor(hdc, colors.text_primary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                ID_STATIC_HINT => {
                    let _ = SetTextColor(hdc, colors.text_secondary);
                    LRESULT(CreateSolidBrush(colors.bg).0 as isize)
                }
                ID_CHECK_ENABLE
                | ID_CHECK_SOUND
                | ID_CHECK_AUTOSTART
                | ID_CHECK_MINIMIZE
                | ID_CHECK_CLOSE_TO_TRAY
                | ID_CHECK_EXCLUDE_INJECTED
                | ID_CHECK_RECORD_ALL => {
                    let _ = SetTextColor(hdc, colors.text_primary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
                _ => {
                    let _ = SetTextColor(hdc, colors.text_primary);
                    LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
                }
            }
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut _);
            let is_dark = theme_is_dark();
            let colors = ThemeColors::new(is_dark);
            let _ = SetTextColor(hdc, colors.text_primary);
            let _ = SetBkColor(hdc, colors.edit_bg);
            LRESULT(CreateSolidBrush(colors.edit_bg).0 as isize)
        }
        WM_CTLCOLORBTN => {
            let is_dark = theme_is_dark();
            let colors = ThemeColors::new(is_dark);
            LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
        }
        WM_SETTINGCHANGE => {
            apply_window_chrome(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;

            if id == ID_BTN_SAVE && code == BN_CLICKED {
                on_save(hwnd);
            }
            if id == ID_BTN_SOUND_TEST && code == BN_CLICKED {
                if let Ok(h) = GetDlgItem(Some(hwnd), ID_EDIT_SOUND_FILE) {
                    let mut buf = [0u16; 512];
                    let len = GetWindowTextW(h, &mut buf) as usize;
                    let file_str = String::from_utf16_lossy(&buf[..len.min(buf.len())])
                        .trim()
                        .to_string();
                    // The test button previews exactly what was typed; failure
                    // falls back to the system beep inside the player.
                    if !file_str.is_empty() {
                        play_path_for_preview(std::path::Path::new(&file_str));
                    }
                }
            }
            if id == ID_BTN_CLEAN_MODE && code == BN_CLICKED {
                if let Some(shared) = SHARED.get() {
                    shared.lock().unwrap().debouncer.cleaning_mode = true;
                }
                update_stats(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
            if id == ID_BTN_OPEN_CPL && code == BN_CLICKED {
                let _ = std::process::Command::new("control.exe")
                    .arg("keyboard")
                    .spawn();
            }
            if id == ID_CHECK_ENABLE && code == BN_CLICKED {
                let enabled = get_check(hwnd, ID_CHECK_ENABLE);
                if let Some(shared) = SHARED.get() {
                    shared.lock().unwrap().enabled = enabled;
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            update_stats(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let close_to_tray = SAVED
                .get()
                .map(|s| s.lock().unwrap().close_to_tray)
                .unwrap_or(true);
            if close_to_tray {
                let _ = ShowWindow(hwnd, SW_HIDE);
            } else {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), 1);
            if let Some(saved) = SAVED.get() {
                let mut s = saved.lock().unwrap();
                if let Some(shared) = SHARED.get() {
                    let g = shared.lock().unwrap();
                    s.total_presses = g.debouncer.total_presses;
                    s.blocked_presses = g.debouncer.blocked_presses;
                }
                config::save(&s);
            }
            remove_tray(hwnd);
            let raw = HOOK.swap(0, Ordering::Relaxed);
            if raw != 0 {
                let _ = UnhookWindowsHookEx(HHOOK(raw as _));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ if msg == WM_UPDATE_EXIT => {
            // Update helper launched: exit so the executable can be replaced.
            // WM_DESTROY persists counters and removes the tray icon.
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ if msg == WM_TRAY => match lparam.0 as u32 {
            WM_LBUTTONDBLCLK => {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                show_tray_menu(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        },
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub(crate) unsafe fn create_window(hinstance: HINSTANCE, init: &Settings) -> HWND {
    let class = w!("KeyboardChatteringFixMain");
    // MAKEINTRESOURCE(1): see add_tray for why the lint is suppressed here.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = LoadIconW(Some(hinstance), PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        hIcon: icon,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let title = to_wide(strings(init.language).title);

    // Modern window dimensions: 480 x 824 outer size
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        480,
        824,
        Some(HWND::default()),
        Some(HMENU::default()),
        Some(hinstance),
        Some(init as *const Settings as *const _),
    )
    .unwrap_or_default();

    if !icon.is_invalid() {
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
    }

    hwnd
}

pub(crate) unsafe fn show_initial(hwnd: HWND, minimized: bool) {
    let _ = ShowWindow(hwnd, if minimized { SW_HIDE } else { SW_SHOW });
    let _ = UpdateWindow(hwnd);
}
