//! UI language selection. Every visible string exists in both languages.

/// UI language. Every visible string exists in both languages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Ko,
    En,
}

impl Language {
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_lowercase().as_str() {
            "en" => Language::En,
            _ => Language::Ko,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Language::Ko => "ko",
            Language::En => "en",
        }
    }

    pub fn combo_index(self) -> usize {
        match self {
            Language::Ko => 0,
            Language::En => 1,
        }
    }

    pub fn from_combo_index(i: usize) -> Self {
        match i {
            1 => Language::En,
            _ => Language::Ko,
        }
    }
}
