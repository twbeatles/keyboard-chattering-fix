//! History tab (event log and per-key summary list view).
mod clock;
mod csv;
mod rows;
mod table;
#[cfg(test)]
mod tests;
mod view;
pub(crate) use csv::csv_snapshot;
pub(crate) use rows::threshold_cell;
pub(crate) use table::{
    apply_language, apply_theme, double_clicked_vk, init, invalidate, on_notify, on_search_changed,
    on_view_changed, refresh, set_columns, setup_list, theme_list, write_cell,
};
