//! Native Win32 presentation layer.
//!
//! - `ids`: control IDs, tray menu IDs, private window messages
//! - `state`: UI-owned state (active language, last saved settings)
//! - `text`: control text/checkbox helpers and full-window retranslation
//! - `stats`: telemetry readout refreshed on the 500 ms timer
//! - `tray`: system-tray icon and context menu
//! - `dialog`: modal dialogs owned by the main window
//! - `update`: tray-driven release-update flow (worker thread + prompts)
//! - `settings_form`: read/validate/persist/apply for the settings card
//! - `paint`: theme colors, fonts, and card-background painting
//! - `controls`: Win32 control factory for window construction
//! - `window`: message dispatch, window lifecycle, teardown
//!
//! Only the window entry points are re-exported; every other module talks
//! to its siblings through `super::`, keeping the Win32 surface contained.

mod controls;
mod dialog;
mod ids;
mod paint;
mod settings_form;
mod state;
mod stats;
mod text;
mod tray;
mod update;
mod window;

pub(crate) use window::{create_window, show_initial};
