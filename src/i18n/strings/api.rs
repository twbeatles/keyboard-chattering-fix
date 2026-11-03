//! Localizable UI strings (Korean / English).
//!
//! Wording rule: describe what the user sees ("a key typed twice"), not the
//! mechanism ("debounce threshold"). Placeholders are positional `{}`.

use super::super::language::Language;
use super::en::EN;
use super::ko::KO;
use super::schema::Strings;

pub fn strings(lang: Language) -> &'static Strings {
    match lang {
        Language::Ko => &KO,
        Language::En => &EN,
    }
}

/// Fill positional `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    for arg in args {
        match rest.find("{}") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(arg);
                rest = &rest[i + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}
