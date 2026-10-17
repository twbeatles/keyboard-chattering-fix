//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};

use super::ids::*;
use crate::i18n::Strings;

/// Logical client size of the main window (scaled by DPI at creation).
pub(crate) const CLIENT_W: i32 = 480;
pub(crate) const CLIENT_H: i32 = 618;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    Home = 0,
    History = 1,
    Settings = 2,
    Advanced = 3,
}

impl Page {
    pub(crate) const ALL: [Page; 4] = [Page::Home, Page::History, Page::Settings, Page::Advanced];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    fn from_index(i: u8) -> Page {
        match i {
            1 => Page::History,
            2 => Page::Settings,
            3 => Page::Advanced,
            _ => Page::Home,
        }
    }

    /// The tab button that selects this page.
    pub(crate) fn tab_id(self) -> i32 {
        ID_TAB_HOME + self as i32
    }

    pub(crate) fn from_tab_id(id: i32) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.tab_id() == id)
    }
}

// Visibility scopes (bit set). A control is shown when the current page's
// bit is set, or — while cleaning mode is active — when CLEANING is set.
pub(crate) const HOME: u8 = 1;
pub(crate) const HISTORY: u8 = 2;
pub(crate) const SETTINGS: u8 = 4;
pub(crate) const ADVANCED: u8 = 8;
pub(crate) const CLEANING: u8 = 16;
/// Every page, hidden while cleaning (tabs).
pub(crate) const PAGES: u8 = HOME | HISTORY | SETTINGS | ADVANCED;
/// Always visible (header, footer).
pub(crate) const EVERY: u8 = PAGES | CLEANING;

/// Control role: decides the window class, font, and colors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Title,
    Subtitle,
    Section,
    Label,
    Hint,
    StatLabel,
    StatValue,
    StatAccent,
    Detail,
    CleanTitle,
    CleanBody,
    Footer,
    Status,
    Edit,
    NumberEdit,
    Combo,
    List,
    Toggle,
    ToggleLarge,
    Button,
    Primary,
    Tab,
    Pill,
}

impl Kind {
    /// Owner-drawn button kinds (painted in `widgets`).
    pub(crate) fn is_owner_drawn(self) -> bool {
        matches!(
            self,
            Kind::Toggle
                | Kind::ToggleLarge
                | Kind::Button
                | Kind::Primary
                | Kind::Tab
                | Kind::Pill
        )
    }

    pub(crate) fn is_toggle(self) -> bool {
        matches!(self, Kind::Toggle | Kind::ToggleLarge)
    }

    pub(crate) fn is_static(self) -> bool {
        matches!(
            self,
            Kind::Title
                | Kind::Subtitle
                | Kind::Section
                | Kind::Label
                | Kind::Hint
                | Kind::StatLabel
                | Kind::StatValue
                | Kind::StatAccent
                | Kind::Detail
                | Kind::CleanTitle
                | Kind::CleanBody
                | Kind::Footer
                | Kind::Status
        )
    }
}

pub(crate) type TextFn = fn(&Strings) -> &'static str;

pub(crate) struct Spec {
    pub id: i32,
    pub kind: Kind,
    pub scope: u8,
    /// Logical x, y, width, height at 96 DPI.
    pub rect: (i32, i32, i32, i32),
    /// Static text binding; `None` for dynamic text (stats, pill, inputs).
    pub text: Option<TextFn>,
    /// Sits on a card (card background) rather than the window background.
    pub on_card: bool,
}

const fn c(
    id: i32,
    kind: Kind,
    scope: u8,
    rect: (i32, i32, i32, i32),
    text: Option<TextFn>,
) -> Spec {
    Spec {
        id,
        kind,
        scope,
        rect,
        text,
        on_card: true,
    }
}

const fn w(
    id: i32,
    kind: Kind,
    scope: u8,
    rect: (i32, i32, i32, i32),
    text: Option<TextFn>,
) -> Spec {
    Spec {
        id,
        kind,
        scope,
        rect,
        text,
        on_card: false,
    }
}

/// Creation order is also the Tab-key order.
pub(crate) static SPECS: &[Spec] = &[
    // Header
    w(
        ID_HEADER_TITLE,
        Kind::Title,
        EVERY,
        (24, 12, 300, 28),
        Some(|s| s.title),
    ),
    w(
        ID_HEADER_SUB,
        Kind::Subtitle,
        EVERY,
        (24, 40, 300, 18),
        Some(|s| s.subtitle),
    ),
    w(ID_PILL, Kind::Pill, EVERY, (330, 18, 134, 30), None),
    // Tabs (track painted at TAB_TRACK)
    w(
        ID_TAB_HOME,
        Kind::Tab,
        PAGES,
        (20, 72, 110, 30),
        Some(|s| s.tab_home),
    ),
    w(
        ID_TAB_HISTORY,
        Kind::Tab,
        PAGES,
        (130, 72, 110, 30),
        Some(|s| s.tab_history),
    ),
    w(
        ID_TAB_SETTINGS,
        Kind::Tab,
        PAGES,
        (240, 72, 110, 30),
        Some(|s| s.tab_settings),
    ),
    w(
        ID_TAB_ADVANCED,
        Kind::Tab,
        PAGES,
        (350, 72, 110, 30),
        Some(|s| s.tab_advanced),
    ),
    // Home: protection card
    c(
        ID_TOGGLE_ENABLE,
        Kind::ToggleLarge,
        HOME,
        (32, 130, 416, 28),
        Some(|s| s.enable_filter),
    ),
    c(
        ID_HOME_DESC,
        Kind::Hint,
        HOME,
        (32, 164, 416, 36),
        Some(|s| s.home_desc),
    ),
    // Home: statistics card
    c(
        ID_STATS_TITLE,
        Kind::Section,
        HOME,
        (32, 236, 300, 20),
        Some(|s| s.card_stats),
    ),
    c(
        ID_STAT_TOTAL_LABEL,
        Kind::StatLabel,
        HOME,
        (32, 264, 124, 18),
        Some(|s| s.stat_total),
    ),
    c(
        ID_STAT_TOTAL_VAL,
        Kind::StatValue,
        HOME,
        (32, 282, 124, 32),
        None,
    ),
    c(
        ID_STAT_BLOCKED_LABEL,
        Kind::StatLabel,
        HOME,
        (174, 264, 124, 18),
        Some(|s| s.stat_blocked),
    ),
    c(
        ID_STAT_BLOCKED_VAL,
        Kind::StatValue,
        HOME,
        (174, 282, 124, 32),
        None,
    ),
    c(
        ID_STAT_RATIO_LABEL,
        Kind::StatLabel,
        HOME,
        (316, 264, 124, 18),
        Some(|s| s.stat_ratio),
    ),
    c(
        ID_STAT_RATIO_VAL,
        Kind::StatAccent,
        HOME,
        (316, 282, 124, 32),
        None,
    ),
    c(ID_STAT_LAST, Kind::Detail, HOME, (32, 324, 416, 18), None),
    c(ID_STAT_TOP, Kind::Detail, HOME, (32, 346, 416, 18), None),
    // Home: tools card
    c(
        ID_TOOLS_TITLE,
        Kind::Section,
        HOME,
        (32, 396, 300, 20),
        Some(|s| s.card_tools),
    ),
    c(
        ID_BTN_CLEAN_MODE,
        Kind::Button,
        HOME,
        (32, 424, 132, 30),
        Some(|s| s.clean_mode_btn),
    ),
    c(
        ID_BTN_RESET_STATS,
        Kind::Button,
        HOME,
        (174, 424, 132, 30),
        Some(|s| s.reset_stats_btn),
    ),
    c(
        ID_BTN_OPEN_CPL,
        Kind::Button,
        HOME,
        (316, 424, 132, 30),
        Some(|s| s.open_cpl_btn),
    ),
    w(
        ID_HOME_TIP,
        Kind::Hint,
        HOME,
        (24, 480, 432, 36),
        Some(|s| s.home_tip),
    ),
    // Cleaning overlay
    c(
        ID_CLEAN_TITLE,
        Kind::CleanTitle,
        CLEANING,
        (32, 146, 416, 30),
        Some(|s| s.clean_title),
    ),
    c(
        ID_CLEAN_BODY,
        Kind::CleanBody,
        CLEANING,
        (40, 188, 400, 64),
        Some(|s| s.clean_body),
    ),
    c(
        ID_BTN_CLEAN_END,
        Kind::Primary,
        CLEANING,
        (160, 270, 160, 38),
        Some(|s| s.clean_end_btn),
    ),
    // History
    w(
        ID_HIST_VIEW,
        Kind::Combo,
        HISTORY,
        (16, 118, 160, 240),
        None,
    ),
    w(
        ID_HIST_SEARCH,
        Kind::Edit,
        HISTORY,
        (184, 118, 170, 25),
        None,
    ),
    w(
        ID_BTN_CLEAR_LOG,
        Kind::Button,
        HISTORY,
        (362, 116, 102, 29),
        Some(|s| s.history_clear_btn),
    ),
    w(ID_HIST_LIST, Kind::List, HISTORY, (16, 152, 448, 388), None),
    w(ID_HIST_INFO, Kind::Hint, HISTORY, (16, 548, 448, 36), None),
    // Settings: sensitivity card
    c(
        ID_THRESH_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 130, 300, 20),
        Some(|s| s.card_threshold),
    ),
    c(
        ID_LABEL_THRESHOLD,
        Kind::Label,
        SETTINGS,
        (32, 161, 130, 20),
        Some(|s| s.threshold_label),
    ),
    c(
        ID_EDIT_THRESHOLD,
        Kind::NumberEdit,
        SETTINGS,
        (166, 158, 64, 25),
        None,
    ),
    c(
        ID_LABEL_UNIT,
        Kind::Label,
        SETTINGS,
        (238, 161, 60, 20),
        Some(|s| s.unit_ms),
    ),
    c(
        ID_THRESH_HINT,
        Kind::Hint,
        SETTINGS,
        (32, 192, 416, 50),
        Some(|s| s.threshold_hint),
    ),
    // Settings: sound card
    c(
        ID_SOUND_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 276, 300, 20),
        Some(|s| s.card_sound),
    ),
    c(
        ID_TOGGLE_SOUND,
        Kind::Toggle,
        SETTINGS,
        (32, 302, 316, 26),
        Some(|s| s.play_sound),
    ),
    c(
        ID_BTN_SOUND_TEST,
        Kind::Button,
        SETTINGS,
        (360, 300, 88, 29),
        Some(|s| s.sound_test),
    ),
    c(
        ID_EDIT_SOUND_FILE,
        Kind::Edit,
        SETTINGS,
        (32, 336, 318, 25),
        None,
    ),
    c(
        ID_BTN_SOUND_BROWSE,
        Kind::Button,
        SETTINGS,
        (360, 334, 88, 29),
        Some(|s| s.sound_browse),
    ),
    // Settings: app card
    c(
        ID_APP_TITLE,
        Kind::Section,
        SETTINGS,
        (32, 390, 300, 20),
        Some(|s| s.card_app),
    ),
    c(
        ID_TOGGLE_AUTOSTART,
        Kind::Toggle,
        SETTINGS,
        (32, 414, 416, 26),
        Some(|s| s.autostart),
    ),
    c(
        ID_TOGGLE_MINIMIZE,
        Kind::Toggle,
        SETTINGS,
        (32, 440, 416, 26),
        Some(|s| s.minimize_on_launch),
    ),
    c(
        ID_TOGGLE_CLOSE_TO_TRAY,
        Kind::Toggle,
        SETTINGS,
        (32, 466, 416, 26),
        Some(|s| s.close_to_tray),
    ),
    c(
        ID_LABEL_LANG,
        Kind::Label,
        SETTINGS,
        (32, 501, 130, 20),
        Some(|s| s.language_label),
    ),
    c(
        ID_COMBO_LANG,
        Kind::Combo,
        SETTINGS,
        (166, 497, 160, 240),
        None,
    ),
    // Advanced: detection card
    c(
        ID_DETECT_TITLE,
        Kind::Section,
        ADVANCED,
        (32, 130, 300, 20),
        Some(|s| s.card_detect),
    ),
    c(
        ID_LABEL_MEASURE,
        Kind::Label,
        ADVANCED,
        (32, 161, 130, 20),
        Some(|s| s.measure_from_label),
    ),
    c(
        ID_COMBO_MEASURE,
        Kind::Combo,
        ADVANCED,
        (166, 157, 282, 240),
        None,
    ),
    c(
        ID_LABEL_MIN_CHATTER,
        Kind::Label,
        ADVANCED,
        (32, 197, 130, 20),
        Some(|s| s.min_chatter_label),
    ),
    c(
        ID_EDIT_MIN_CHATTER,
        Kind::NumberEdit,
        ADVANCED,
        (166, 194, 64, 25),
        None,
    ),
    c(
        ID_LABEL_MIN_UNIT,
        Kind::Label,
        ADVANCED,
        (238, 197, 210, 20),
        Some(|s| s.min_chatter_unit),
    ),
    c(
        ID_MIN_HINT,
        Kind::Hint,
        ADVANCED,
        (32, 226, 416, 36),
        Some(|s| s.min_chatter_hint),
    ),
    c(
        ID_TOGGLE_EXCLUDE_INJECTED,
        Kind::Toggle,
        ADVANCED,
        (32, 268, 416, 26),
        Some(|s| s.exclude_injected),
    ),
    // Advanced: history card
    c(
        ID_LOG_TITLE,
        Kind::Section,
        ADVANCED,
        (32, 330, 300, 20),
        Some(|s| s.card_log),
    ),
    c(
        ID_TOGGLE_RECORD_ALL,
        Kind::Toggle,
        ADVANCED,
        (32, 356, 416, 26),
        Some(|s| s.record_all_keys),
    ),
    c(
        ID_LABEL_LOG_LIMIT,
        Kind::Label,
        ADVANCED,
        (32, 393, 130, 20),
        Some(|s| s.log_limit_label),
    ),
    c(
        ID_EDIT_LOG_LIMIT,
        Kind::NumberEdit,
        ADVANCED,
        (166, 390, 64, 25),
        None,
    ),
    c(
        ID_LABEL_LOG_UNIT,
        Kind::Label,
        ADVANCED,
        (238, 393, 210, 20),
        Some(|s| s.log_limit_unit),
    ),
    c(
        ID_LOG_NOTE,
        Kind::Hint,
        ADVANCED,
        (32, 424, 416, 36),
        Some(|s| s.log_note),
    ),
    // Save row (Settings + Advanced)
    w(
        ID_BTN_SAVE,
        Kind::Primary,
        SETTINGS | ADVANCED,
        (16, 542, 120, 34),
        Some(|s| s.save),
    ),
    w(
        ID_SAVE_STATUS,
        Kind::Status,
        SETTINGS | ADVANCED,
        (148, 550, 316, 20),
        None,
    ),
    // Footer
    w(ID_FOOTER, Kind::Footer, EVERY, (16, 592, 448, 18), None),
];

/// Rounded track behind the tab buttons.
pub(crate) const TAB_TRACK: (i32, i32, i32, i32) = (16, 68, 448, 38);

/// Card backgrounds painted for each page (logical rects).
pub(crate) fn cards(page: Page, cleaning: bool) -> &'static [(i32, i32, i32, i32)] {
    if cleaning {
        return &[(16, 118, 448, 220)];
    }
    match page {
        Page::Home => &[(16, 118, 448, 96), (16, 224, 448, 150), (16, 384, 448, 82)],
        Page::History => &[],
        Page::Settings => &[
            (16, 118, 448, 136),
            (16, 264, 448, 108),
            (16, 378, 448, 152),
        ],
        Page::Advanced => &[(16, 118, 448, 190), (16, 318, 448, 150)],
    }
}

/// Vertical dividers between the three Home statistics (x, y0, y1).
pub(crate) const STAT_DIVIDERS: [(i32, i32, i32); 2] = [(165, 266, 312), (307, 266, 312)];

pub(crate) fn spec(id: i32) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

pub(crate) fn is_visible(scope: u8, page: Page, cleaning: bool) -> bool {
    if cleaning {
        scope & CLEANING != 0
    } else {
        scope & page.bit() != 0
    }
}

// --- Runtime state (UI thread) -------------------------------------------

static PAGE: AtomicU8 = AtomicU8::new(0);
/// Whether the cleaning overlay is what the window currently shows.
static SHOWING_CLEANING: AtomicBool = AtomicBool::new(false);
static DPI: AtomicI32 = AtomicI32::new(96);

pub(crate) fn current_page() -> Page {
    Page::from_index(PAGE.load(Ordering::Relaxed))
}

pub(crate) fn set_current_page(page: Page) {
    PAGE.store(page as u8, Ordering::Relaxed);
}

pub(crate) fn showing_cleaning() -> bool {
    SHOWING_CLEANING.load(Ordering::Relaxed)
}

pub(crate) fn set_showing_cleaning(on: bool) {
    SHOWING_CLEANING.store(on, Ordering::Relaxed);
}

pub(crate) fn set_dpi(dpi: i32) {
    DPI.store(dpi.max(96), Ordering::Relaxed);
}

/// Scale a logical (96-DPI) length to device pixels.
pub(crate) fn sc(v: i32) -> i32 {
    let dpi = DPI.load(Ordering::Relaxed);
    (v * dpi + 48) / 96
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{strings, Language};

    #[test]
    fn control_ids_are_unique() {
        for (i, a) in SPECS.iter().enumerate() {
            for b in &SPECS[i + 1..] {
                assert_ne!(a.id, b.id, "duplicate control id {}", a.id);
            }
        }
    }

    #[test]
    fn every_control_fits_inside_the_client_area() {
        for s in SPECS {
            let (x, y, w, h) = s.rect;
            // Combo heights include the drop-down list, so only check y.
            let bottom = if s.kind == Kind::Combo { y + 26 } else { y + h };
            assert!(
                x >= 0 && x + w <= CLIENT_W,
                "id {} overflows horizontally",
                s.id
            );
            assert!(
                y >= 0 && bottom <= CLIENT_H,
                "id {} overflows vertically",
                s.id
            );
        }
    }

    #[test]
    fn card_controls_sit_inside_a_card_of_their_page() {
        for s in SPECS.iter().filter(|s| s.on_card) {
            let (x, y, w, h) = s.rect;
            let h = if s.kind == Kind::Combo { 26 } else { h };
            let pages: Vec<(Page, bool)> = if s.scope == CLEANING {
                vec![(Page::Home, true)]
            } else {
                Page::ALL
                    .into_iter()
                    .filter(|p| s.scope & p.bit() != 0)
                    .map(|p| (p, false))
                    .collect()
            };
            assert!(!pages.is_empty(), "id {} belongs to no page", s.id);
            for (page, cleaning) in pages {
                let inside = cards(page, cleaning).iter().any(|&(cx, cy, cw, ch)| {
                    x >= cx && y >= cy && x + w <= cx + cw && y + h <= cy + ch
                });
                assert!(inside, "id {} is outside every card on {page:?}", s.id);
            }
        }
    }

    #[test]
    fn controls_on_the_same_page_do_not_overlap() {
        let overlaps = |a: &Spec, b: &Spec| {
            let (ax, ay, aw, ah) = a.rect;
            let (bx, by, bw, bh) = b.rect;
            let ah = if a.kind == Kind::Combo { 26 } else { ah };
            let bh = if b.kind == Kind::Combo { 26 } else { bh };
            ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
        };
        for (i, a) in SPECS.iter().enumerate() {
            for b in &SPECS[i + 1..] {
                if a.scope & b.scope != 0 {
                    assert!(!overlaps(a, b), "ids {} and {} overlap", a.id, b.id);
                }
            }
        }
    }

    #[test]
    fn save_button_is_reachable_from_both_settings_pages_only() {
        let save = spec(ID_BTN_SAVE).unwrap();
        assert!(is_visible(save.scope, Page::Settings, false));
        assert!(is_visible(save.scope, Page::Advanced, false));
        assert!(!is_visible(save.scope, Page::Home, false));
        assert!(!is_visible(save.scope, Page::History, false));
        assert!(!is_visible(save.scope, Page::Settings, true));
    }

    #[test]
    fn cleaning_overlay_hides_tabs_but_keeps_header() {
        let tab = spec(ID_TAB_HOME).unwrap();
        let pill = spec(ID_PILL).unwrap();
        let end = spec(ID_BTN_CLEAN_END).unwrap();
        assert!(!is_visible(tab.scope, Page::Home, true));
        assert!(is_visible(pill.scope, Page::Home, true));
        assert!(is_visible(end.scope, Page::History, true));
        assert!(!is_visible(end.scope, Page::Home, false));
    }

    #[test]
    fn tabs_map_to_pages_and_back() {
        for page in Page::ALL {
            assert_eq!(Page::from_tab_id(page.tab_id()), Some(page));
            assert_eq!(spec(page.tab_id()).unwrap().kind, Kind::Tab);
        }
        assert_eq!(Page::from_tab_id(ID_BTN_SAVE), None);
    }

    #[test]
    fn bound_texts_are_non_empty_in_both_languages() {
        for lang in [Language::Ko, Language::En] {
            let s = strings(lang);
            for spec in SPECS {
                if let Some(text) = spec.text {
                    assert!(
                        !text(s).trim().is_empty(),
                        "id {} empty in {lang:?}",
                        spec.id
                    );
                }
            }
        }
    }

    #[test]
    fn scaling_rounds_to_nearest_pixel() {
        set_dpi(144);
        assert_eq!(sc(10), 15);
        assert_eq!(sc(CLIENT_W), 720);
        set_dpi(120);
        assert_eq!(sc(10), 13);
        set_dpi(96);
        assert_eq!(sc(17), 17);
    }
}
