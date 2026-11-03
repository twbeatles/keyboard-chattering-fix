//! Declarative control table (page, rect, text) and page state.
mod kinds;
mod page;
mod query;
mod specs;
mod state;
#[cfg(test)]
mod tests;
pub(crate) use kinds::Kind;
pub(crate) use page::{CLIENT_H, CLIENT_W, Page};
pub(crate) use query::{cards, is_visible, spec};
pub(crate) use specs::{SPECS, STAT_DIVIDERS, TAB_TRACK};
pub(crate) use state::{
    current_page, sc, set_current_page, set_dpi, set_showing_cleaning, showing_cleaning,
};
