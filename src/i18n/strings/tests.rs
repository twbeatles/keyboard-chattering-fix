//! Localizable UI strings (Korean / English).
//!
//! Wording rule: describe what the user sees ("a key typed twice"), not the
//! mechanism ("debounce threshold"). Placeholders are positional `{}`.

use super::super::language::Language;

use super::api::{fill, strings};

fn placeholders(s: &str) -> usize {
    s.matches("{}").count()
}

fn others() -> impl Iterator<Item = Language> {
    Language::ALL
        .iter()
        .copied()
        .filter(|l| *l != Language::FALLBACK)
}

/// Every field of every language is filled in. A forgotten placeholder
/// such as an empty string would otherwise show up as a blank label.
#[test]
fn every_text_of_every_language_is_filled_in() {
    for lang in Language::ALL {
        let s = strings(*lang);
        for (name, text) in s.texts() {
            assert!(!text.trim().is_empty(), "{lang:?}.{name} is empty");
        }
        for (name, items) in s.lists() {
            assert!(!items.is_empty(), "{lang:?}.{name} has no items");
            assert!(
                items.iter().all(|i| !i.trim().is_empty()),
                "{lang:?}.{name}"
            );
        }
    }
}

/// A translation must carry the same `{}` placeholders as the English
/// reference, otherwise a value is silently dropped or misplaced.
#[test]
fn every_language_keeps_the_reference_placeholders() {
    let reference = strings(Language::FALLBACK);
    for lang in others() {
        let s = strings(lang);
        for ((name, want), (_, got)) in reference.texts().into_iter().zip(s.texts()) {
            assert_eq!(
                placeholders(got),
                placeholders(want),
                "{lang:?}.{name}: {got}"
            );
        }
    }
}

/// Every field is translated, not copied from English. Short brand-like
/// or numeric texts may legitimately match (`Home`/`홈` aside), so only
/// sentence-length text is required to differ.
#[test]
fn long_texts_are_actually_translated() {
    let reference = strings(Language::FALLBACK);
    for lang in others() {
        let s = strings(lang);
        for ((name, want), (_, got)) in reference.texts().into_iter().zip(s.texts()) {
            if want.chars().count() > 24 {
                assert_ne!(got, want, "{lang:?}.{name} is still the English text");
            }
        }
    }
}

#[test]
fn option_lists_have_same_length_in_every_language() {
    let reference = strings(Language::FALLBACK);
    for lang in others() {
        let s = strings(lang);
        for ((name, want), (_, got)) in reference.lists().into_iter().zip(s.lists()) {
            assert_eq!(got.len(), want.len(), "{lang:?}.{name}");
        }
    }
    assert_eq!(reference.history_views.len(), 3);
    assert_eq!(reference.measure_options.len(), 2);
}

#[test]
fn tray_tip_fits_notify_icon_buffer() {
    // NOTIFYICONDATAW.szTip holds 128 UTF-16 units including the NUL.
    for lang in Language::ALL.iter().copied() {
        let s = strings(lang);
        for tip in [s.tray_tip_on, s.tray_tip_off, s.tray_tip_paused] {
            assert!(tip.encode_utf16().count() < 128, "{tip}");
        }
        assert!(s.tray_balloon.encode_utf16().count() < 256);
    }
}

#[test]
fn fill_replaces_in_order_and_tolerates_mismatch() {
    assert_eq!(fill("{} ({}ms)", &["A", "12"]), "A (12ms)");
    // Extra args are ignored, missing args leave the placeholder.
    assert_eq!(fill("{}", &["x", "y"]), "x");
    assert_eq!(fill("{} {}", &["x"]), "x {}");
    assert_eq!(fill("none", &[]), "none");
}
