use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex, OnceLock,
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE},
            Gdi::*,
        },
        System::{
            Registry::{
                RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
                REG_VALUE_TYPE,
            },
            SystemServices::{SS_CENTER, SS_LEFT},
        },
        UI::{
            Controls::{BST_CHECKED, BST_UNCHECKED},
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::*,
        },
    },
};

use crate::{
    config::{self, Settings},
    debounce::MeasureFrom,
    i18n::{strings, Language},
    play_notification_sound, SHARED,
};

// Control IDs.
const ID_HEADER_TITLE: i32 = 100;
const ID_HEADER_SUB: i32 = 101;
const ID_HEADER_BADGE: i32 = 102;
const ID_CLEAN_BANNER: i32 = 103;

const ID_CARD1_TITLE: i32 = 200;
const ID_STAT_TOTAL_LABEL: i32 = 201;
const ID_STAT_TOTAL_VAL: i32 = 202;
const ID_STAT_CHATTER_LABEL: i32 = 203;
const ID_STAT_CHATTER_VAL: i32 = 204;
const ID_STAT_RATIO_LABEL: i32 = 205;
const ID_STAT_RATIO_VAL: i32 = 206;
const ID_STATIC_LAST: i32 = 207;
const ID_STATIC_TOP_CHATTER: i32 = 208;

const ID_CARD2_TITLE: i32 = 300;
const ID_LABEL_THRESHOLD: i32 = 301;
const ID_EDIT_THRESHOLD: i32 = 302;
const ID_LABEL_UNIT: i32 = 303;
const ID_CHECK_ENABLE: i32 = 304;
const ID_CHECK_SOUND: i32 = 305;
const ID_LABEL_SOUND_FILE: i32 = 306;
const ID_EDIT_SOUND_FILE: i32 = 307;
const ID_BTN_SOUND_TEST: i32 = 308;
const ID_CHECK_AUTOSTART: i32 = 309;
const ID_CHECK_MINIMIZE: i32 = 310;
const ID_CHECK_CLOSE_TO_TRAY: i32 = 311;
const ID_CHECK_RECORD_ALL: i32 = 312;

const ID_CARD3_TITLE: i32 = 400;
const ID_BTN_CLEAN_MODE: i32 = 401;
const ID_BTN_OPEN_CPL: i32 = 402;

const ID_CARD4_TITLE: i32 = 500;
const ID_LABEL_MEASURE: i32 = 501;
const ID_COMBO_MEASURE: i32 = 502;
const ID_LABEL_LANG: i32 = 503;
const ID_COMBO_LANG: i32 = 504;

const ID_BTN_SAVE: i32 = 601;
const ID_STATIC_HINT: i32 = 602;

// Tray menu IDs.
const MENU_SHOW: usize = 1001;
const MENU_TOGGLE: usize = 1002;
const MENU_CLEANING: usize = 1003;
const MENU_EXIT: usize = 1004;
const MENU_UPDATE: usize = 1005;

const WM_TRAY: u32 = WM_APP + 1;
/// Posted from the update worker thread after the helper is launched; the
/// window must be destroyed on the UI thread (this process has to exit so
/// the helper can replace the executable file).
const WM_UPDATE_EXIT: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;

static LANG: AtomicU8 = AtomicU8::new(0);
static SAVED: OnceLock<Mutex<Settings>> = OnceLock::new();

#[inline]
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

struct ThemeColors {
    _is_dark: bool,
    bg: COLORREF,
    card_bg: COLORREF,
    card_border: COLORREF,
    text_primary: COLORREF,
    text_secondary: COLORREF,
    accent: COLORREF,
    status_active_bg: COLORREF,
    status_active_fg: COLORREF,
    status_paused_bg: COLORREF,
    status_paused_fg: COLORREF,
    cleaning_banner_bg: COLORREF,
    cleaning_banner_fg: COLORREF,
    edit_bg: COLORREF,
}

impl ThemeColors {
    fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                _is_dark: true,
                bg: rgb(30, 30, 30),
                card_bg: rgb(40, 40, 40),
                card_border: rgb(58, 58, 58),
                text_primary: rgb(245, 245, 245),
                text_secondary: rgb(165, 165, 165),
                accent: rgb(76, 194, 255),
                status_active_bg: rgb(35, 75, 45),
                status_active_fg: rgb(108, 203, 95),
                status_paused_bg: rgb(65, 65, 65),
                status_paused_fg: rgb(180, 180, 180),
                cleaning_banner_bg: rgb(120, 60, 20),
                cleaning_banner_fg: rgb(255, 210, 140),
                edit_bg: rgb(50, 50, 50),
            }
        } else {
            Self {
                _is_dark: false,
                bg: rgb(243, 243, 243),
                card_bg: rgb(255, 255, 255),
                card_border: rgb(228, 228, 228),
                text_primary: rgb(24, 24, 24),
                text_secondary: rgb(110, 110, 110),
                accent: rgb(0, 120, 212),
                status_active_bg: rgb(223, 246, 221),
                status_active_fg: rgb(15, 123, 15),
                status_paused_bg: rgb(235, 235, 235),
                status_paused_fg: rgb(100, 100, 100),
                cleaning_banner_bg: rgb(255, 244, 206),
                cleaning_banner_fg: rgb(157, 93, 0),
                edit_bg: rgb(255, 255, 255),
            }
        }
    }
}

fn is_system_dark_mode() -> bool {
    unsafe {
        let key_path = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let val_name = to_wide("AppsUseLightTheme");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_ok()
        {
            let mut data = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let mut type_ = REG_VALUE_TYPE(0);
            let res = RegQueryValueExW(
                hkey,
                PCWSTR(val_name.as_ptr()),
                None,
                Some(&mut type_),
                Some(&mut data as *mut _ as *mut _),
                Some(&mut size),
            );
            let _ = RegCloseKey(hkey);
            if res.is_ok() {
                return data == 0;
            }
        }
    }
    true
}

fn vk_name(vk: u8) -> String {
    match vk {
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x10 => "Shift".to_string(),
        0x11 => "Ctrl".to_string(),
        0x12 => "Alt".to_string(),
        0x13 => "Pause".to_string(),
        0x14 => "CapsLock".to_string(),
        0x1B => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x23 => "End".to_string(),
        0x24 => "Home".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x2C => "PrintScreen".to_string(),
        0x2D => "Insert".to_string(),
        0x2E => "Delete".to_string(),
        0x30..=0x39 | 0x41..=0x5A => (vk as char).to_string(),
        0x5B => "LWin".to_string(),
        0x5C => "RWin".to_string(),
        0x5D => "Apps".to_string(),
        0x5F => "Sleep".to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x6A => "Num*".to_string(),
        0x6B => "Num+".to_string(),
        0x6D => "Num-".to_string(),
        0x6E => "Num.".to_string(),
        0x6F => "Num/".to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x90 => "NumLock".to_string(),
        0x91 => "ScrollLock".to_string(),
        0xA0 => "LShift".to_string(),
        0xA1 => "RShift".to_string(),
        0xA2 => "LCtrl".to_string(),
        0xA3 => "RCtrl".to_string(),
        0xA4 => "LAlt".to_string(),
        0xA5 => "RAlt".to_string(),
        0xA6 => "BrowserBack".to_string(),
        0xA7 => "BrowserForward".to_string(),
        0xA8 => "BrowserRefresh".to_string(),
        0xA9 => "BrowserStop".to_string(),
        0xAA => "BrowserSearch".to_string(),
        0xAB => "BrowserFavorites".to_string(),
        0xAC => "BrowserHome".to_string(),
        0xAD => "VolumeMute".to_string(),
        0xAE => "VolumeDown".to_string(),
        0xAF => "VolumeUp".to_string(),
        0xB0 => "MediaNext".to_string(),
        0xB1 => "MediaPrev".to_string(),
        0xB2 => "MediaStop".to_string(),
        0xB3 => "MediaPlayPause".to_string(),
        0xB4 => "LaunchMail".to_string(),
        0xB5 => "LaunchMedia".to_string(),
        0xB6 => "LaunchApp1".to_string(),
        0xB7 => "LaunchApp2".to_string(),
        0xBA => ";".to_string(),
        0xBB => "=".to_string(),
        0xBC => ",".to_string(),
        0xBD => "-".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xC0 => "`".to_string(),
        0xDB => "[".to_string(),
        0xDC => "\\".to_string(),
        0xDD => "]".to_string(),
        0xDE => "'".to_string(),
        _ => format!("0x{vk:02X}"),
    }
}

fn current_lang() -> Language {
    if LANG.load(Ordering::Relaxed) == 1 {
        Language::En
    } else {
        Language::Ko
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn create_font(size_pt: i32, weight: i32) -> HFONT {
    let font_names = [
        "Segoe UI Variable Text",
        "Segoe UI",
        "맑은 고딕",
        "Malgun Gothic",
    ];
    for name in font_names {
        let wname = to_wide(name);
        let hf = CreateFontW(
            -size_pt,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(wname.as_ptr()),
        );
        if !hf.is_invalid() {
            return hf;
        }
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0 as *mut _)
}

struct GuiFonts {
    title: HFONT,
    subtitle: HFONT,
    section: HFONT,
    stat_val: HFONT,
    body: HFONT,
    badge: HFONT,
}

unsafe impl Send for GuiFonts {}
unsafe impl Sync for GuiFonts {}

static FONTS: OnceLock<GuiFonts> = OnceLock::new();

fn get_fonts() -> &'static GuiFonts {
    FONTS.get_or_init(|| unsafe {
        GuiFonts {
            title: create_font(18, FW_BOLD.0 as i32),
            subtitle: create_font(12, FW_NORMAL.0 as i32),
            section: create_font(13, FW_SEMIBOLD.0 as i32),
            stat_val: create_font(16, FW_BOLD.0 as i32),
            body: create_font(12, FW_NORMAL.0 as i32),
            badge: create_font(11, FW_BOLD.0 as i32),
        }
    })
}

// Win32 control factory: the parameter list mirrors `CreateWindowExW`, so
// splitting it would only add indirection at every call site.
#[allow(clippy::too_many_arguments)]
unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w_: i32,
    h: i32,
    id: i32,
    hinstance: HINSTANCE,
    font: HFONT,
) -> HWND {
    let t = to_wide(text);
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        PCWSTR(t.as_ptr()),
        style | WS_CHILD | WS_VISIBLE,
        x,
        y,
        w_,
        h,
        parent,
        HMENU(id as usize as *mut _),
        hinstance,
        None,
    )
    .unwrap_or_default();

    if !hwnd.is_invalid() && !font.is_invalid() {
        let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    }
    hwnd
}

unsafe fn set_text(parent: HWND, id: i32, s: &str) {
    if let Ok(h) = GetDlgItem(parent, id) {
        let v = to_wide(s);
        let _ = SetWindowTextW(h, PCWSTR(v.as_ptr()));
    }
}

unsafe fn set_check(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = GetDlgItem(parent, id) {
        let _ = SendMessageW(
            h,
            BM_SETCHECK,
            WPARAM(if on {
                BST_CHECKED.0 as usize
            } else {
                BST_UNCHECKED.0 as usize
            }),
            LPARAM(0),
        );
    }
}

unsafe fn get_check(parent: HWND, id: i32) -> bool {
    if let Ok(h) = GetDlgItem(parent, id) {
        return SendMessageW(h, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == BST_CHECKED.0 as isize;
    }
    false
}

unsafe fn apply_language(hwnd: HWND) {
    let s = strings(current_lang());
    let t = to_wide(s.title);
    let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));

    set_text(hwnd, ID_HEADER_TITLE, s.title);
    set_text(hwnd, ID_HEADER_SUB, s.subtitle);

    set_text(hwnd, ID_CARD1_TITLE, s.card_telemetry);
    set_text(hwnd, ID_STAT_TOTAL_LABEL, s.stat_total);
    set_text(hwnd, ID_STAT_CHATTER_LABEL, s.stat_blocked);
    set_text(hwnd, ID_STAT_RATIO_LABEL, s.stat_ratio);

    set_text(hwnd, ID_CARD2_TITLE, s.card_settings);
    set_text(hwnd, ID_LABEL_THRESHOLD, s.threshold_label);
    set_text(hwnd, ID_LABEL_UNIT, s.threshold_unit);
    set_text(hwnd, ID_CHECK_ENABLE, s.enable_filter);
    set_text(hwnd, ID_CHECK_SOUND, s.play_sound);
    set_text(hwnd, ID_LABEL_SOUND_FILE, s.sound_file_label);
    set_text(hwnd, ID_BTN_SOUND_TEST, s.sound_test);
    set_text(hwnd, ID_CHECK_AUTOSTART, s.autostart);
    set_text(hwnd, ID_CHECK_MINIMIZE, s.minimize_on_launch);
    set_text(hwnd, ID_CHECK_CLOSE_TO_TRAY, s.close_to_tray);
    set_text(hwnd, ID_CHECK_RECORD_ALL, s.record_all_keys);

    set_text(hwnd, ID_CARD3_TITLE, s.card_tools);
    set_text(hwnd, ID_BTN_CLEAN_MODE, s.clean_mode_btn);
    set_text(hwnd, ID_BTN_OPEN_CPL, s.open_cpl_btn);

    set_text(hwnd, ID_CARD4_TITLE, s.card_options);
    set_text(hwnd, ID_LABEL_MEASURE, s.measure_from_label);
    set_text(hwnd, ID_LABEL_LANG, s.language_label);

    set_text(hwnd, ID_BTN_SAVE, s.save);
    set_text(hwnd, ID_STATIC_HINT, s.tray_hint);

    if let Ok(combo_m) = GetDlgItem(hwnd, ID_COMBO_MEASURE) {
        let cur = SendMessageW(combo_m, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let _ = SendMessageW(combo_m, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        let opt1 = to_wide(s.measure_release);
        let opt2 = to_wide(s.measure_press);
        SendMessageW(
            combo_m,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(opt1.as_ptr() as isize),
        );
        SendMessageW(
            combo_m,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(opt2.as_ptr() as isize),
        );
        let sel = if cur >= 0 { cur as usize } else { 0 };
        SendMessageW(combo_m, CB_SETCURSEL, WPARAM(sel), LPARAM(0));
    }

    update_stats(hwnd);
}

unsafe fn update_stats(hwnd: HWND) {
    let s = strings(current_lang());
    if let Some(shared) = SHARED.get() {
        let g = shared.lock().unwrap();

        // 1. Cleaning mode banner toggle
        if let Ok(banner) = GetDlgItem(hwnd, ID_CLEAN_BANNER) {
            if g.debouncer.cleaning_mode {
                let _ = ShowWindow(banner, SW_SHOW);
                set_text(hwnd, ID_CLEAN_BANNER, s.clean_mode_banner);
            } else {
                let _ = ShowWindow(banner, SW_HIDE);
            }
        }

        // 2. Status badge
        let badge_text = if g.enabled {
            s.status_active
        } else {
            s.status_paused
        };
        set_text(hwnd, ID_HEADER_BADGE, badge_text);

        // 3. Telemetry counters
        set_text(
            hwnd,
            ID_STAT_TOTAL_VAL,
            &format!("{}", g.debouncer.total_presses),
        );
        set_text(
            hwnd,
            ID_STAT_CHATTER_VAL,
            &format!("{}", g.debouncer.blocked_presses),
        );
        set_text(
            hwnd,
            ID_STAT_RATIO_VAL,
            &format!("{:.1}%", g.debouncer.chatter_ratio()),
        );

        // 4. Last blocked key
        match g.debouncer.last_blocked {
            Some((vk, gap)) => {
                let msg = s.last_blocked_fmt.replacen("{}", &vk_name(vk), 1).replacen(
                    "{}",
                    &gap.as_millis().to_string(),
                    1,
                );
                set_text(hwnd, ID_STATIC_LAST, &msg);
            }
            None => set_text(hwnd, ID_STATIC_LAST, s.last_blocked_none),
        }

        // 5. Top chattered key
        match g.debouncer.top_chattered_key() {
            Some((vk, blocked, ratio)) => {
                let msg = s
                    .top_chattered_fmt
                    .replacen("{}", &vk_name(vk), 1)
                    .replacen("{}", &blocked.to_string(), 1)
                    .replacen("{:.1}", &format!("{:.1}", ratio), 1);
                set_text(hwnd, ID_STATIC_TOP_CHATTER, &msg);
            }
            None => set_text(hwnd, ID_STATIC_TOP_CHATTER, s.top_chattered_none),
        }
    }
}

unsafe fn add_tray(hwnd: HWND) {
    let s = strings(current_lang());
    let hinstance =
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
    // MAKEINTRESOURCE(1): icon resource ID, not a real pointer. Clippy's
    // `dangling()` suggestion would change the address and break loading.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = LoadIconW(hinstance, PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let tip = to_wide(s.tray_tip);
    let n = tip.len().min(nid.szTip.len());
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let s = strings(current_lang());
    let enabled = SHARED
        .get()
        .map(|g| g.lock().unwrap().enabled)
        .unwrap_or(true);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let item = |id: usize, text: &str| {
        let v = to_wide(text);
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(v.as_ptr()));
    };
    item(MENU_SHOW, s.menu_show);
    item(
        MENU_TOGGLE,
        if enabled {
            s.menu_disable
        } else {
            s.menu_enable
        },
    );
    item(MENU_CLEANING, s.menu_cleaning);
    item(MENU_UPDATE, s.menu_update);
    item(MENU_EXIT, s.menu_exit);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    match picked.0 as usize {
        MENU_SHOW => {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        MENU_TOGGLE => {
            if let Some(g) = SHARED.get() {
                let mut g = g.lock().unwrap();
                g.enabled = !g.enabled;
                set_check(hwnd, ID_CHECK_ENABLE, g.enabled);
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
        MENU_CLEANING => {
            if let Some(g) = SHARED.get() {
                g.lock().unwrap().debouncer.cleaning_mode = true;
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            update_stats(hwnd);
            let _ = InvalidateRect(hwnd, None, false);
        }
        MENU_UPDATE => {
            spawn_update_check(hwnd);
        }
        MENU_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

unsafe fn msg_box(
    hwnd: HWND,
    title: &str,
    text: &str,
    flags: MESSAGEBOX_STYLE,
) -> MESSAGEBOX_RESULT {
    let t = to_wide(title);
    let m = to_wide(text);
    MessageBoxW(hwnd, PCWSTR(m.as_ptr()), PCWSTR(t.as_ptr()), flags)
}

/// Pure classification of an update-check result for the tray flow, split
/// out so the branches are unit-testable without windows or network.
#[derive(Debug)]
enum UpdatePrompt {
    UpToDate,
    Available(crate::updater::UpdateManifest),
    Failed(String),
}

fn classify_update_result(
    result: Result<crate::updater::UpdateManifest, crate::updater::UpdateError>,
) -> UpdatePrompt {
    match result {
        Err(crate::updater::UpdateError::NoUpdate) => UpdatePrompt::UpToDate,
        Ok(manifest) => UpdatePrompt::Available(manifest),
        Err(err) => UpdatePrompt::Failed(err.to_string()),
    }
}

/// Manual update check from the tray menu. Network I/O runs on a worker
/// thread so the message loop never blocks; the actual file swap is done by
/// `kcfix-updater.exe` after this process exits.
fn spawn_update_check(hwnd: HWND) {
    let raw_hwnd = hwnd.0 as usize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw_hwnd as *mut _);
        let s = strings(current_lang());
        if !crate::updater::try_begin_update() {
            unsafe {
                msg_box(
                    hwnd,
                    s.update_title,
                    s.update_busy,
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            return;
        }
        match classify_update_result(crate::updater::check_for_update()) {
            UpdatePrompt::UpToDate => {
                crate::updater::end_update();
                let text = s.update_latest.replacen("{}", crate::config::VERSION, 1);
                unsafe {
                    msg_box(hwnd, s.update_title, &text, MB_OK | MB_ICONINFORMATION);
                }
            }
            UpdatePrompt::Available(manifest) => {
                let text = s
                    .update_available
                    .replacen("{}", &manifest.version, 1)
                    .replacen("{}", crate::config::VERSION, 1);
                let answer =
                    unsafe { msg_box(hwnd, s.update_title, &text, MB_YESNO | MB_ICONINFORMATION) };
                if answer != IDYES {
                    crate::updater::end_update();
                    return;
                }
                match crate::updater::prepare_update(&manifest) {
                    Ok(staged) => {
                        if let Err(err) = crate::updater::launch_helper(&staged) {
                            crate::updater::discard_staged(&staged);
                            crate::updater::end_update();
                            unsafe {
                                msg_box(
                                    hwnd,
                                    s.update_failed_title,
                                    &err.to_string(),
                                    MB_OK | MB_ICONERROR,
                                );
                            }
                            return;
                        }
                        // The helper owns the swap from here; this process
                        // must exit so the executable file can be replaced.
                        // (Single-flight flag intentionally left set.)
                        unsafe {
                            msg_box(
                                hwnd,
                                s.update_title,
                                s.update_applying,
                                MB_OK | MB_ICONINFORMATION,
                            );
                            let _ = PostMessageW(hwnd, WM_UPDATE_EXIT, WPARAM(0), LPARAM(0));
                        }
                    }
                    Err(err) => {
                        crate::updater::end_update();
                        unsafe {
                            msg_box(
                                hwnd,
                                s.update_failed_title,
                                &err.to_string(),
                                MB_OK | MB_ICONERROR,
                            );
                        }
                    }
                }
            }
            UpdatePrompt::Failed(message) => {
                crate::updater::end_update();
                unsafe {
                    msg_box(hwnd, s.update_failed_title, &message, MB_OK | MB_ICONERROR);
                }
            }
        }
    });
}

unsafe fn on_save(hwnd: HWND) {
    let mut threshold = SHARED
        .get()
        .map(|g| g.lock().unwrap().debouncer.threshold_ms())
        .unwrap_or(50);
    if let Ok(h) = GetDlgItem(hwnd, ID_EDIT_THRESHOLD) {
        let mut buf = [0u16; 32];
        let len = GetWindowTextW(h, &mut buf) as usize;
        if let Ok(n) = String::from_utf16_lossy(&buf[..len.min(buf.len())])
            .trim()
            .parse::<u64>()
        {
            threshold = n.clamp(1, 1000);
        }
        let back = to_wide(&threshold.to_string());
        let _ = SetWindowTextW(h, PCWSTR(back.as_ptr()));
    }

    let sound_file = if let Ok(h) = GetDlgItem(hwnd, ID_EDIT_SOUND_FILE) {
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(h, &mut buf) as usize;
        let s = String::from_utf16_lossy(&buf[..len.min(buf.len())])
            .trim()
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    } else {
        None
    };

    let enabled = get_check(hwnd, ID_CHECK_ENABLE);
    let sound = get_check(hwnd, ID_CHECK_SOUND);
    let autostart = get_check(hwnd, ID_CHECK_AUTOSTART);
    let minimize = get_check(hwnd, ID_CHECK_MINIMIZE);
    let close_to_tray = get_check(hwnd, ID_CHECK_CLOSE_TO_TRAY);
    let record_all = get_check(hwnd, ID_CHECK_RECORD_ALL);

    let measure = if let Ok(h) = GetDlgItem(hwnd, ID_COMBO_MEASURE) {
        let sel = SendMessageW(h, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        if sel == 1 {
            MeasureFrom::Press
        } else {
            MeasureFrom::Release
        }
    } else {
        MeasureFrom::Release
    };

    let lang = if let Ok(h) = GetDlgItem(hwnd, ID_COMBO_LANG) {
        let sel = SendMessageW(h, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        Language::from_combo_index(if sel >= 0 { sel as usize } else { 0 })
    } else {
        current_lang()
    };

    config::set_autostart(autostart);
    let mut new_settings = config::load();
    new_settings.threshold_ms = threshold;
    new_settings.enabled = enabled;
    new_settings.play_sound = sound;
    new_settings.sound_file = sound_file.clone();
    new_settings.minimize_on_launch = minimize;
    new_settings.close_to_tray = close_to_tray;
    new_settings.record_all_keys = record_all;
    new_settings.measure_from = measure;
    new_settings.language = lang;

    if let Some(g) = SHARED.get() {
        let mut g = g.lock().unwrap();
        g.debouncer.set_threshold_ms(threshold);
        g.debouncer.set_measure_from(measure);
        g.debouncer.record_all_keys = record_all;
        g.enabled = enabled;
        g.play_sound = sound;
        // Validate once here; the hook must never touch the filesystem.
        g.sound_file = config::resolve_sound_file(sound_file.as_deref());
        new_settings.total_presses = g.debouncer.total_presses;
        new_settings.blocked_presses = g.debouncer.blocked_presses;
    }
    LANG.store(if lang == Language::En { 1 } else { 0 }, Ordering::Relaxed);
    config::save(&new_settings);
    if let Some(saved) = SAVED.get() {
        *saved.lock().unwrap() = new_settings;
    }
    apply_language(hwnd);
    let _ = InvalidateRect(hwnd, None, true);
}

unsafe fn draw_rounded_card(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, brush);
    let old_pen = SelectObject(hdc, pen);

    let _ = RoundRect(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(brush);
    let _ = DeleteObject(pen);
}

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

            let is_dark = is_system_dark_mode();
            let dark_attr: i32 = if is_dark { 1 } else { 0 };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_attr as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );

            // Windows 11 Rounded Window Corners: DWMWA_WINDOW_CORNER_PREFERENCE (33) = ROUND (2)
            let corner: i32 = 2;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33),
                &corner as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );

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
            if let Ok(b) = GetDlgItem(hwnd, ID_CLEAN_BANNER) {
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

            // 6. Card 4: Advanced Preferences (y: 516..600)
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
                WPARAM(0),
                LPARAM(opt1.as_ptr() as isize),
            );
            SendMessageW(
                combo_m,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(opt2.as_ptr() as isize),
            );
            SendMessageW(
                combo_m,
                CB_SETCURSEL,
                WPARAM(if init.measure_from == MeasureFrom::Press {
                    1
                } else {
                    0
                }),
                LPARAM(0),
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
                    WPARAM(0),
                    LPARAM(v.as_ptr() as isize),
                );
            }
            SendMessageW(
                combo_l,
                CB_SETCURSEL,
                WPARAM(init.language.combo_index()),
                LPARAM(0),
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

            // 7. Bottom Actions (y: 610..)
            child(
                hwnd,
                w!("BUTTON"),
                "",
                WINDOW_STYLE(BS_DEFPUSHBUTTON as u32 | WS_TABSTOP.0),
                16,
                612,
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
                656,
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
            set_check(hwnd, ID_CHECK_AUTOSTART, config::autostart_enabled());

            apply_language(hwnd);
            add_tray(hwnd);
            let _ = SetTimer(hwnd, 1, 500, None);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let width = rc.right - rc.left;
            let height = rc.bottom - rc.top;

            let is_dark = is_system_dark_mode();
            let colors = ThemeColors::new(is_dark);

            // Double buffering
            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
            let old_bmp = SelectObject(mem_dc, mem_bmp);

            // Fill canvas
            let bg_brush = CreateSolidBrush(colors.bg);
            FillRect(mem_dc, &rc, bg_brush);
            let _ = DeleteObject(bg_brush);

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
            let old_pen = SelectObject(mem_dc, div_pen);
            let _ = MoveToEx(mem_dc, 156, 138, None);
            let _ = LineTo(mem_dc, 156, 180);
            let _ = MoveToEx(mem_dc, 296, 138, None);
            let _ = LineTo(mem_dc, 296, 180);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(div_pen);

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

            // Draw Card 4 (Preferences): 16, 516, 448, 600
            let c4 = RECT {
                left: 16,
                top: 516,
                right: 448,
                bottom: 600,
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

            let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
            SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(mem_bmp);
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let c_hwnd = HWND(lparam.0 as *mut _);
            let id = GetWindowLongW(c_hwnd, GWL_ID);
            let is_dark = is_system_dark_mode();
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
                | ID_LABEL_LANG => {
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
            let is_dark = is_system_dark_mode();
            let colors = ThemeColors::new(is_dark);
            let _ = SetTextColor(hdc, colors.text_primary);
            let _ = SetBkColor(hdc, colors.edit_bg);
            LRESULT(CreateSolidBrush(colors.edit_bg).0 as isize)
        }
        WM_CTLCOLORBTN => {
            let is_dark = is_system_dark_mode();
            let colors = ThemeColors::new(is_dark);
            LRESULT(CreateSolidBrush(colors.card_bg).0 as isize)
        }
        WM_SETTINGCHANGE => {
            let is_dark = is_system_dark_mode();
            let dark_attr: i32 = if is_dark { 1 } else { 0 };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_attr as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;

            if id == ID_BTN_SAVE && code == BN_CLICKED {
                on_save(hwnd);
            }
            if id == ID_BTN_SOUND_TEST && code == BN_CLICKED {
                if let Ok(h) = GetDlgItem(hwnd, ID_EDIT_SOUND_FILE) {
                    let mut buf = [0u16; 512];
                    let len = GetWindowTextW(h, &mut buf) as usize;
                    let file_str = String::from_utf16_lossy(&buf[..len.min(buf.len())])
                        .trim()
                        .to_string();
                    // The test button previews exactly what was typed; failure
                    // falls back to the system beep inside the player.
                    play_notification_sound(
                        (!file_str.is_empty()).then(|| std::path::Path::new(&file_str)),
                    );
                }
            }
            if id == ID_BTN_CLEAN_MODE && code == BN_CLICKED {
                if let Some(shared) = SHARED.get() {
                    shared.lock().unwrap().debouncer.cleaning_mode = true;
                }
                update_stats(hwnd);
                let _ = InvalidateRect(hwnd, None, true);
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
                let _ = InvalidateRect(hwnd, None, false);
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
            let _ = KillTimer(hwnd, 1);
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
            let raw = crate::HOOK.swap(0, Ordering::Relaxed);
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

pub unsafe fn create_window(hinstance: HINSTANCE, init: &Settings) -> HWND {
    let class = w!("KeyboardChatteringFixMain");
    // MAKEINTRESOURCE(1): see add_tray for why the lint is suppressed here.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = LoadIconW(hinstance, PCWSTR(1 as *const u16))
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

    // Modern window dimensions: 480 x 728 outer size
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        480,
        728,
        HWND::default(),
        HMENU::default(),
        hinstance,
        Some(init as *const Settings as *const _),
    )
    .unwrap_or_default();

    if !icon.is_invalid() {
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(icon.0 as isize),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(icon.0 as isize),
        );
    }

    hwnd
}

pub unsafe fn show_initial(hwnd: HWND, minimized: bool) {
    let _ = ShowWindow(hwnd, if minimized { SW_HIDE } else { SW_SHOW });
    let _ = UpdateWindow(hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> crate::updater::UpdateManifest {
        crate::updater::UpdateManifest {
            version: "0.2.0".to_string(),
            tag: "v0.2.0".to_string(),
            artifact_url: "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe".to_string(),
            sha256: "0".repeat(64),
            size: 1024,
        }
    }

    #[test]
    fn classify_maps_no_update_to_up_to_date() {
        assert!(matches!(
            classify_update_result(Err(crate::updater::UpdateError::NoUpdate)),
            UpdatePrompt::UpToDate
        ));
    }

    #[test]
    fn classify_carries_manifest_for_available() {
        match classify_update_result(Ok(sample_manifest())) {
            UpdatePrompt::Available(m) => assert_eq!(m.version, "0.2.0"),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn classify_preserves_error_message() {
        match classify_update_result(Err(crate::updater::UpdateError::Message(
            "boom".to_string(),
        ))) {
            UpdatePrompt::Failed(message) => assert_eq!(message, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
