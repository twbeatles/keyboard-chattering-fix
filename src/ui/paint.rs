//! Look and feel: theme colors, system dark-mode detection, font cache,
//! and card-background painting.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    OnceLock,
};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{COLORREF, HWND, RECT},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE},
            Gdi::*,
        },
        System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
            REG_VALUE_TYPE,
        },
    },
};

use super::text::to_wide;

#[inline]
pub(crate) fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

pub(crate) struct ThemeColors {
    pub(crate) _is_dark: bool,
    pub(crate) bg: COLORREF,
    pub(crate) card_bg: COLORREF,
    pub(crate) card_border: COLORREF,
    pub(crate) text_primary: COLORREF,
    pub(crate) text_secondary: COLORREF,
    pub(crate) accent: COLORREF,
    pub(crate) status_active_bg: COLORREF,
    pub(crate) status_active_fg: COLORREF,
    pub(crate) status_paused_bg: COLORREF,
    pub(crate) status_paused_fg: COLORREF,
    pub(crate) cleaning_banner_bg: COLORREF,
    pub(crate) cleaning_banner_fg: COLORREF,
    pub(crate) edit_bg: COLORREF,
}

impl ThemeColors {
    pub(crate) fn new(is_dark: bool) -> Self {
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

/// Cached dark-mode flag: 0 = light, 1 = dark, 2 = unknown.
/// The registry is read once and re-read only when Windows broadcasts a
/// setting change, so per-control painting never performs registry I/O.
/// (Previously every `WM_CTLCOLOR*` did a full open/query/close cycle
/// per control, per paint.)
static THEME_DARK: AtomicU8 = AtomicU8::new(2);

/// Re-read the system theme into the cache. Call on startup and on
/// `WM_SETTINGCHANGE`; painting paths must use [`theme_is_dark`] instead.
pub(crate) fn refresh_theme() {
    THEME_DARK.store(read_system_dark_mode() as u8, Ordering::Relaxed);
}

/// Cached system-theme lookup for paint paths: no registry I/O.
pub(crate) fn theme_is_dark() -> bool {
    match THEME_DARK.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => {
            refresh_theme();
            THEME_DARK.load(Ordering::Relaxed) == 1
        }
    }
}

fn read_system_dark_mode() -> bool {
    unsafe {
        let key_path = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let val_name = to_wide("AppsUseLightTheme");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            None,
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

pub(crate) struct GuiFonts {
    pub(crate) title: HFONT,
    pub(crate) subtitle: HFONT,
    pub(crate) section: HFONT,
    pub(crate) stat_val: HFONT,
    pub(crate) body: HFONT,
    pub(crate) badge: HFONT,
}

unsafe impl Send for GuiFonts {}
unsafe impl Sync for GuiFonts {}

static FONTS: OnceLock<GuiFonts> = OnceLock::new();

pub(crate) fn get_fonts() -> &'static GuiFonts {
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
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(wname.as_ptr()),
        );
        if !hf.is_invalid() {
            return hf;
        }
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0 as *mut _)
}

pub(crate) unsafe fn draw_rounded_card(
    hdc: HDC,
    rect: &RECT,
    bg: COLORREF,
    border: COLORREF,
    radius: i32,
) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, brush.into());
    let old_pen = SelectObject(hdc, pen.into());

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
    let _ = DeleteObject(brush.into());
    let _ = DeleteObject(pen.into());
}

/// Apply the Windows 11 dark title bar and rounded corners to a new window.
pub(crate) unsafe fn apply_window_chrome(hwnd: HWND) {
    refresh_theme();
    let is_dark = theme_is_dark();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_themes_differ() {
        let dark = ThemeColors::new(true);
        let light = ThemeColors::new(false);
        assert_ne!(dark.bg.0, light.bg.0);
        assert_ne!(dark.text_primary.0, light.text_primary.0);
        assert_ne!(dark.accent.0, light.accent.0);
    }

    #[test]
    fn cached_theme_matches_registry_read() {
        // The cache must never disagree with the underlying registry value.
        refresh_theme();
        assert_eq!(theme_is_dark(), read_system_dark_mode());
        // A second lookup hits the cache and returns the same answer.
        assert_eq!(theme_is_dark(), read_system_dark_mode());
    }
}
