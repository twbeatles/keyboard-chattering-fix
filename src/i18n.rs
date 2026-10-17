//! Internationalization: language selection and localizable strings.

mod language;
mod strings;

pub use language::{system_language, system_locale_name, Language, LanguagePref};
pub use strings::{fill, strings, Strings};
