//! Keys tab (per-key windows, mouse switch, threshold advice).
mod actions;
mod model;
mod table;
#[cfg(test)]
mod tests;
pub(crate) use actions::{
    apply_all_suggestions, apply_mouse_threshold, apply_override, clear_key_stats, clear_override,
    on_combo_changed, on_enter, on_mouse_toggled, on_notify, on_wheel_toggled, select_key,
};
pub(crate) use table::{apply_language, apply_theme, init, invalidate, refresh};
