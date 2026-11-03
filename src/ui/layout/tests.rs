//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use super::super::ids::*;

use super::kinds::{Kind, Spec};
use super::page::{CLEANING, CLIENT_H, CLIENT_W, Page};
use super::query::{cards, is_visible, spec};
use super::specs::{SPECS, TAB_TRACK};
use super::state::{sc, set_dpi};
use crate::i18n::{Language, strings};

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
    // The Keys tab applies each change on its own button.
    assert!(!is_visible(save.scope, Page::Keys, false));
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
fn tabs_sit_side_by_side_inside_the_track() {
    let (tx, ty, tw, th) = TAB_TRACK;
    let mut tabs: Vec<(i32, i32, i32, i32)> = Page::ALL
        .iter()
        .map(|p| spec(p.tab_id()).unwrap().rect)
        .collect();
    tabs.sort();
    for pair in tabs.windows(2) {
        assert_eq!(pair[0].0 + pair[0].2, pair[1].0, "tabs must touch");
    }
    for (x, y, w, h) in tabs {
        assert!(x >= tx && y >= ty && x + w <= tx + tw && y + h <= ty + th);
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
