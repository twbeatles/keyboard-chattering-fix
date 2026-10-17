//! Internationalization: language selection and localizable strings.

mod language;
mod strings;

pub use language::Language;
pub use strings::{fill, strings, Strings, LANGUAGE_NAMES};
