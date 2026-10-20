//! Look and feel: theme colors, system dark-mode detection, font cache,
//! and card-background painting.

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, RECT},
        Graphics::{
            Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute},
            Gdi::*,
        },
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        },
    },
    core::PCWSTR,
};

use windows::{
    Win32::UI::{Controls::SetWindowTheme, WindowsAndMessaging::GetDlgItem},
    core::w,
};

use super::{
    layout::{Kind, SPECS, sc},
    text::to_wide,
};

#[inline]
pub(crate) fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

pub(crate) struct ThemeColors {
    pub(crate) bg: COLORREF,
    pub(crate) card_bg: COLORREF,
    pub(crate) card_border: COLORREF,
    pub(crate) text_primary: COLORREF,
    pub(crate) text_secondary: COLORREF,
    pub(crate) accent: COLORREF,
    pub(crate) accent_pressed: COLORREF,
    pub(crate) on_accent: COLORREF,
    pub(crate) status_active_bg: COLORREF,
    pub(crate) status_active_fg: COLORREF,
    pub(crate) status_paused_bg: COLORREF,
    pub(crate) status_paused_fg: COLORREF,
    pub(crate) warn: COLORREF,
    pub(crate) edit_bg: COLORREF,
    pub(crate) tab_track: COLORREF,
    pub(crate) tab_selected: COLORREF,
    pub(crate) button_bg: COLORREF,
    pub(crate) button_pressed: COLORREF,
    pub(crate) button_border: COLORREF,
    pub(crate) toggle_off: COLORREF,
}

impl ThemeColors {
    pub(crate) fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                bg: rgb(32, 32, 32),
                card_bg: rgb(43, 43, 43),
                card_border: rgb(60, 60, 60),
                text_primary: rgb(242, 242, 242),
                text_secondary: rgb(170, 170, 170),
                accent: rgb(96, 205, 255),
                accent_pressed: rgb(72, 170, 220),
                on_accent: rgb(0, 0, 0),
                status_active_bg: rgb(30, 70, 40),
                status_active_fg: rgb(108, 203, 95),
                status_paused_bg: rgb(70, 62, 30),
                status_paused_fg: rgb(252, 225, 0),
                warn: rgb(255, 153, 120),
                edit_bg: rgb(52, 52, 52),
                tab_track: rgb(40, 40, 40),
                tab_selected: rgb(64, 64, 64),
                button_bg: rgb(58, 58, 58),
                button_pressed: rgb(46, 46, 46),
                button_border: rgb(74, 74, 74),
                toggle_off: rgb(160, 160, 160),
            }
        } else {
            Self {
                bg: rgb(243, 243, 243),
                card_bg: rgb(255, 255, 255),
                card_border: rgb(229, 229, 229),
                text_primary: rgb(26, 26, 26),
                text_secondary: rgb(96, 96, 96),
                accent: rgb(0, 103, 192),
                accent_pressed: rgb(0, 84, 160),
                on_accent: rgb(255, 255, 255),
                status_active_bg: rgb(223, 246, 221),
                status_active_fg: rgb(16, 124, 16),
                status_paused_bg: rgb(255, 244, 206),
                status_paused_fg: rgb(157, 93, 0),
                warn: rgb(196, 43, 28),
                edit_bg: rgb(255, 255, 255),
                tab_track: rgb(232, 232, 232),
                tab_selected: rgb(255, 255, 255),
                button_bg: rgb(251, 251, 251),
                button_pressed: rgb(235, 235, 235),
                button_border: rgb(208, 208, 208),
                toggle_off: rgb(118, 118, 118),
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

/// Cached background brushes for `WM_CTLCOLOR*` handlers, keyed by color.
/// Windows never deletes the brush such a handler returns, so creating one
/// per message leaks GDI handles over long sessions (ISSUE-002). Entries are
/// only ever touched on the UI thread; the `Send`/`Sync` impls document that.
struct BrushEntry(u32, HBRUSH);
unsafe impl Send for BrushEntry {}
unsafe impl Sync for BrushEntry {}

static BRUSH_CACHE: OnceLock<Mutex<Vec<BrushEntry>>> = OnceLock::new();

/// Return a cached brush for `color`, creating it on first use. Replaces the
/// previous per-message `CreateSolidBrush` in every `WM_CTLCOLOR*` handler.
pub(crate) unsafe fn cached_brush(color: COLORREF) -> HBRUSH {
    unsafe {
        let mut cache = BRUSH_CACHE
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap();
        if let Some(entry) = cache.iter().find(|entry| entry.0 == color.0) {
            return entry.1;
        }
        let brush = CreateSolidBrush(color);
        cache.push(BrushEntry(color.0, brush));
        brush
    }
}

/// Delete all cached brushes. Called whenever the theme may have changed so
/// stale colors are never reused (and their handles are released).
pub(crate) unsafe fn clear_brush_cache() {
    unsafe {
        if let Some(cache) = BRUSH_CACHE.get() {
            let mut cache = cache.lock().unwrap();
            for entry in cache.drain(..) {
                let _ = DeleteObject(entry.1.into());
            }
        }
    }
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
    pub(crate) section: HFONT,
    pub(crate) stat_val: HFONT,
    pub(crate) body: HFONT,
    pub(crate) small: HFONT,
    pub(crate) badge: HFONT,
}

unsafe impl Send for GuiFonts {}
unsafe impl Sync for GuiFonts {}

static FONTS: OnceLock<GuiFonts> = OnceLock::new();

/// Fonts are created once, after `layout::set_dpi`, at the scaled size.
pub(crate) fn get_fonts() -> &'static GuiFonts {
    FONTS.get_or_init(|| unsafe {
        GuiFonts {
            title: create_font(sc(20), FW_BOLD.0 as i32),
            section: create_font(sc(14), FW_SEMIBOLD.0 as i32),
            stat_val: create_font(sc(24), FW_SEMIBOLD.0 as i32),
            body: create_font(sc(13), FW_NORMAL.0 as i32),
            small: create_font(sc(12), FW_NORMAL.0 as i32),
            badge: create_font(sc(12), FW_SEMIBOLD.0 as i32),
        }
    })
}

/// Font for a control role.
pub(crate) fn font_for(kind: Kind) -> HFONT {
    let f = get_fonts();
    match kind {
        Kind::Title | Kind::CleanTitle => f.title,
        Kind::Section | Kind::ToggleLarge => f.section,
        Kind::StatValue | Kind::StatAccent => f.stat_val,
        Kind::Subtitle | Kind::Hint | Kind::Footer | Kind::Status | Kind::StatLabel => f.small,
        Kind::Pill => f.badge,
        _ => f.body,
    }
}

/// Text color and background for a static control role.
pub(crate) fn static_colors(kind: Kind, on_card: bool, c: &ThemeColors) -> (COLORREF, COLORREF) {
    let bg = if on_card { c.card_bg } else { c.bg };
    let fg = match kind {
        Kind::Subtitle | Kind::Hint | Kind::Footer | Kind::StatLabel | Kind::Status => {
            c.text_secondary
        }
        Kind::StatAccent => c.accent,
        Kind::CleanTitle => c.status_paused_fg,
        _ => c.text_primary,
    };
    (fg, bg)
}

unsafe fn create_font(height_px: i32, weight: i32) -> HFONT {
    unsafe {
        // Segoe UI first; Korean glyphs fall back to Malgun Gothic via font
        // linking, so mixed Korean/Latin text keeps Latin digits in Segoe UI.
        let font_names = ["Segoe UI", "맑은 고딕", "Malgun Gothic"];
        for name in font_names {
            let wname = to_wide(name);
            let hf = CreateFontW(
                -height_px,
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
}

/// Logical (96-DPI) rect to device pixels.
pub(crate) fn scaled_rect((x, y, w, h): (i32, i32, i32, i32)) -> RECT {
    RECT {
        left: sc(x),
        top: sc(y),
        right: sc(x + w),
        bottom: sc(y + h),
    }
}

pub(crate) unsafe fn draw_rounded_card(
    hdc: HDC,
    rect: &RECT,
    bg: COLORREF,
    border: COLORREF,
    radius: i32,
) {
    unsafe {
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
}

/// Apply the Windows 11 dark title bar and rounded corners to a new window.
pub(crate) unsafe fn apply_window_chrome(hwnd: HWND) {
    unsafe {
        // The theme may have changed: drop cached brushes with it.
        clear_brush_cache();
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
}

/// Re-skin native child controls for the current theme. Owner-drawn and
/// static controls follow `ThemeColors` on their own; edits and combos need
/// the system dark-mode class to darken their frames and drop-downs.
pub(crate) unsafe fn apply_control_themes(hwnd: HWND) {
    unsafe {
        let dark = theme_is_dark();
        for spec in SPECS {
            if !matches!(spec.kind, Kind::Edit | Kind::NumberEdit | Kind::Combo) {
                continue;
            }
            if let Ok(h) = GetDlgItem(Some(hwnd), spec.id) {
                let _ = if dark {
                    SetWindowTheme(h, w!("DarkMode_CFD"), PCWSTR::null())
                } else {
                    SetWindowTheme(h, PCWSTR::null(), PCWSTR::null())
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctlcolor_brushes_are_cached_per_color() {
        unsafe {
            // Same color: the same handle, no new GDI object (ISSUE-002).
            let a = cached_brush(rgb(11, 22, 33));
            let b = cached_brush(rgb(11, 22, 33));
            assert!(!a.is_invalid());
            assert_eq!(a.0, b.0);
            // Different color: a different live handle.
            let c = cached_brush(rgb(44, 55, 66));
            assert_ne!(a.0, c.0);
            // Clearing drops the cache; fetching again still yields a brush.
            clear_brush_cache();
            assert!(!cached_brush(rgb(11, 22, 33)).is_invalid());
            clear_brush_cache();
        }
    }

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
