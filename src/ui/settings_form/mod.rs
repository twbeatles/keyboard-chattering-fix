//! Settings tab (form load/save, autosave, sound preview).
mod fields;
mod persist;
mod save;
mod sound;
#[cfg(test)]
mod tests;
pub(crate) use fields::{load_into_form, mark_dirty, tick};
pub(crate) use persist::{
    autosave_tick, persist_live, set_key_override, set_key_overrides, set_mouse_filter,
    set_mouse_threshold,
};
pub(crate) use save::on_save;
pub(crate) use sound::{browse_sound, on_language_changed, preview_sound};
