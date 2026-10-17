//! UI language registry and the user's language preference.
//!
//! # Adding a language
//! 1. Add a variant to [`Language`] and list it in [`Language::ALL`].
//! 2. Give it a `code` and `native_name` below (the compiler flags both
//!    `match`es).
//! 3. Add its `Strings` table in `strings.rs` and a `match` arm in
//!    `strings()` (compiler-enforced), plus the `match` arms in
//!    `updater::error` (also compiler-enforced).
//! 4. `cargo test`: the registry tests compare every language against the
//!    English reference (placeholder counts, list lengths, non-empty text),
//!    and the language picker lists it automatically.
//!
//! Detection and persistence need no change: the system locale is matched
//! by its primary subtag (`ja-JP` -> `ja`) against `code`.

/// A UI language the program ships strings for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Ko,
    En,
}

impl Language {
    /// Every shipped language, in picker order.
    pub const ALL: &'static [Language] = &[Language::Ko, Language::En];

    /// Used when the system language is not shipped (and as the reference
    /// translation the others are tested against).
    pub const FALLBACK: Language = Language::En;

    /// Lowercase ISO 639-1 code, also the value stored in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Language::Ko => "ko",
            Language::En => "en",
        }
    }

    /// Name in the language itself, so it never changes with the UI language.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::Ko => "한국어",
            Language::En => "English",
        }
    }

    /// Position in [`Language::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Option<Language> {
        Self::ALL.get(i).copied()
    }

    /// Match a language tag by its primary subtag, case-insensitively:
    /// `ko`, `KO`, `ko-KR`, `ko_KR` all give Korean. `None` when the
    /// language is not shipped.
    pub fn from_code(code: &str) -> Option<Language> {
        let primary = code.trim().split(['-', '_']).next()?.to_ascii_lowercase();
        Self::ALL.iter().copied().find(|l| l.code() == primary)
    }

    /// Language for a Windows locale name such as `ko-KR` or `de-DE`;
    /// anything not shipped gets [`Language::FALLBACK`].
    pub fn from_locale_name(name: &str) -> Language {
        Self::from_code(name).unwrap_or(Self::FALLBACK)
    }
}

/// What the user chose in the language picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LanguagePref {
    /// Follow the Windows display language (default).
    #[default]
    Auto,
    /// A language picked explicitly; stays put whatever Windows uses.
    Fixed(Language),
}

impl LanguagePref {
    /// Settings-file value: `auto`, or a language code. Unknown or empty
    /// values mean `Auto`, so a hand-edited typo never pins a wrong language.
    pub fn from_setting(value: &str) -> LanguagePref {
        let v = value.trim();
        if v.eq_ignore_ascii_case("auto") {
            return LanguagePref::Auto;
        }
        Language::from_code(v).map_or(LanguagePref::Auto, LanguagePref::Fixed)
    }

    pub fn setting_value(self) -> &'static str {
        match self {
            LanguagePref::Auto => "auto",
            LanguagePref::Fixed(l) => l.code(),
        }
    }

    /// The language to show, given the one detected from Windows.
    pub fn resolve(self, system: Language) -> Language {
        match self {
            LanguagePref::Auto => system,
            LanguagePref::Fixed(l) => l,
        }
    }

    /// Picker row: 0 is "Automatic", then one row per shipped language.
    pub fn combo_index(self) -> usize {
        match self {
            LanguagePref::Auto => 0,
            LanguagePref::Fixed(l) => 1 + l.index(),
        }
    }

    pub fn from_combo_index(i: usize) -> LanguagePref {
        i.checked_sub(1)
            .and_then(Language::from_index)
            .map_or(LanguagePref::Auto, LanguagePref::Fixed)
    }
}

/// The language Windows shows its UI in, reduced to a shipped language:
/// Korean for a Korean system, otherwise [`Language::FALLBACK`] (English).
/// Only the first (primary) display language counts, matching what the
/// user sees in the rest of Windows.
pub fn system_language() -> Language {
    system_locale_name().map_or(Language::FALLBACK, |name| Language::from_locale_name(&name))
}

/// The locale name `system_language` was derived from (`ko-KR`), for
/// diagnostics such as `--check-language`.
///
/// `KCFIX_LOCALE` (e.g. `de-DE`) overrides the detection: for reproducing a
/// report from another PC and for testing the non-Korean path.
pub fn system_locale_name() -> Option<String> {
    locale_override(std::env::var("KCFIX_LOCALE").ok()).or_else(locale_name_from_windows)
}

/// A usable override value: trimmed and non-empty.
fn locale_override(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(windows)]
fn locale_name_from_windows() -> Option<String> {
    use windows::{
        core::PWSTR,
        Win32::Globalization::{
            GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
        },
    };
    unsafe {
        // Display (UI) languages, most preferred first, as a double-NUL
        // terminated list. Ask for the size, then for the data.
        let (mut count, mut len) = (0u32, 0u32);
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_ok()
            && len > 1
        {
            let mut buf = vec![0u16; len as usize];
            if GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
            )
            .is_ok()
            {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                if end > 0 {
                    return Some(String::from_utf16_lossy(&buf[..end]));
                }
            }
        }
        // Regional format locale as a second opinion if the list is unusable.
        let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        let n = GetUserDefaultLocaleName(&mut name);
        (n > 1).then(|| String::from_utf16_lossy(&name[..(n as usize - 1)]))
    }
}

#[cfg(not(windows))]
fn locale_name_from_windows() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_codes_and_names_are_unique_and_round_trip() {
        for (i, a) in Language::ALL.iter().enumerate() {
            assert!(!a.code().is_empty() && a.code() == a.code().to_ascii_lowercase());
            assert!(!a.native_name().is_empty());
            assert_eq!(a.index(), i);
            assert_eq!(Language::from_index(i), Some(*a));
            assert_eq!(Language::from_code(a.code()), Some(*a));
            for b in &Language::ALL[i + 1..] {
                assert_ne!(a.code(), b.code());
                assert_ne!(a.native_name(), b.native_name());
            }
        }
        assert!(Language::ALL.contains(&Language::FALLBACK));
        assert_eq!(Language::from_index(Language::ALL.len()), None);
    }

    #[test]
    fn locale_names_map_by_primary_subtag() {
        assert_eq!(Language::from_locale_name("ko-KR"), Language::Ko);
        assert_eq!(Language::from_locale_name("ko"), Language::Ko);
        assert_eq!(Language::from_locale_name("KO_kr"), Language::Ko);
        assert_eq!(Language::from_locale_name(" ko-KR "), Language::Ko);
        assert_eq!(Language::from_locale_name("en-US"), Language::En);
        assert_eq!(Language::from_locale_name("en-GB"), Language::En);
        // Not shipped -> English, including scripts and look-alike prefixes.
        for other in [
            "ja-JP",
            "de-DE",
            "zh-Hans-CN",
            "fr-FR",
            "kok-IN",
            "",
            "-",
            "x",
        ] {
            assert_eq!(
                Language::from_locale_name(other),
                Language::FALLBACK,
                "{other}"
            );
        }
    }

    #[test]
    fn locale_override_ignores_blank_values() {
        assert_eq!(locale_override(None), None);
        assert_eq!(locale_override(Some("  ".into())), None);
        assert_eq!(
            locale_override(Some(" de-DE ".into())),
            Some("de-DE".into())
        );
    }

    #[test]
    fn preference_parses_the_settings_value() {
        assert_eq!(LanguagePref::from_setting("auto"), LanguagePref::Auto);
        assert_eq!(LanguagePref::from_setting(" AUTO "), LanguagePref::Auto);
        assert_eq!(LanguagePref::from_setting(""), LanguagePref::Auto);
        // Legacy files (v0.2.x) stored the language code directly.
        assert_eq!(
            LanguagePref::from_setting("ko"),
            LanguagePref::Fixed(Language::Ko)
        );
        assert_eq!(
            LanguagePref::from_setting("EN"),
            LanguagePref::Fixed(Language::En)
        );
        // Typos and unshipped languages never pin a wrong language.
        assert_eq!(LanguagePref::from_setting("klingon"), LanguagePref::Auto);
        assert_eq!(LanguagePref::from_setting("ja"), LanguagePref::Auto);
        for pref in [
            LanguagePref::Auto,
            LanguagePref::Fixed(Language::Ko),
            LanguagePref::Fixed(Language::En),
        ] {
            assert_eq!(LanguagePref::from_setting(pref.setting_value()), pref);
        }
    }

    #[test]
    fn auto_follows_the_system_and_fixed_ignores_it() {
        assert_eq!(LanguagePref::Auto.resolve(Language::Ko), Language::Ko);
        assert_eq!(LanguagePref::Auto.resolve(Language::En), Language::En);
        assert_eq!(
            LanguagePref::Fixed(Language::En).resolve(Language::Ko),
            Language::En
        );
        assert_eq!(
            LanguagePref::Fixed(Language::Ko).resolve(Language::En),
            Language::Ko
        );
        assert_eq!(LanguagePref::default(), LanguagePref::Auto);
    }

    #[test]
    fn picker_rows_put_automatic_first_and_round_trip() {
        assert_eq!(LanguagePref::Auto.combo_index(), 0);
        assert_eq!(LanguagePref::from_combo_index(0), LanguagePref::Auto);
        for l in Language::ALL {
            let pref = LanguagePref::Fixed(*l);
            assert_eq!(pref.combo_index(), 1 + l.index());
            assert_eq!(LanguagePref::from_combo_index(pref.combo_index()), pref);
        }
        // A stale selection beyond the list falls back to Automatic.
        assert_eq!(LanguagePref::from_combo_index(99), LanguagePref::Auto);
    }

    #[cfg(windows)]
    #[test]
    fn system_language_is_detected_from_windows() {
        // Whatever this machine uses, detection must yield a shipped language
        // and agree with the locale name Windows reports.
        let lang = system_language();
        assert!(Language::ALL.contains(&lang));
        let name = system_locale_name().expect("Windows reports a UI language");
        assert_eq!(lang, Language::from_locale_name(&name), "{name}");
    }
}
