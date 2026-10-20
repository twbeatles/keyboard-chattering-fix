//! Internationalization: language selection and localizable strings.

mod language;
mod strings;

pub use language::{Language, LanguagePref, system_language, system_locale_name};
pub use strings::{Strings, fill, strings};
