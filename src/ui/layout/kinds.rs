//! Declarative window layout: every child control, the page it belongs
//! to, its logical (96-DPI) rectangle, and its text binding.
//!
//! Construction, retranslation, theming, color lookup, and page switching
//! all iterate this one table, so adding a control is a single entry here
//! instead of edits scattered across five handlers. The table is pure data
//! and unit-tested without any window.

use crate::i18n::Strings;

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
