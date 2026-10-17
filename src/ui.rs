//! Native Win32 presentation layer.
//!
//! - `layout`: declarative control table (page, rect, text) and page state
//! - `ids`: control IDs, menu command IDs, private window messages
//! - `state`: UI-owned state (active language, last saved settings)
//! - `text`: control text helpers and full-window retranslation
//! - `widgets`: owner-drawn toggles, buttons, tabs, and the status pill
//! - `stats`: Home statistics refreshed on the 500 ms timer
//! - `history`: History tab (event log and per-key summary list view)
//! - `keys`: virtual-key names and the key filter
//! - `commands`: actions shared by the menu bar, tray, and buttons
//! - `menu`: window menu bar
//! - `tray`: system-tray icon, tooltip, balloon, and context menu
//! - `dialog`: modal dialogs owned by the main window
//! - `update`: release-update flow (worker thread + prompts)
//! - `settings_form`: load/validate/persist/apply for the settings tabs
//! - `paint`: theme colors, fonts, and card-background painting
//! - `controls`: Win32 control factory for window construction
//! - `window`: message dispatch, window lifecycle, teardown
//!
//! Only the window entry points are re-exported; every other module talks
//! to its siblings through `super::`, keeping the Win32 surface contained.

mod commands;
mod controls;
mod dialog;
mod history;
mod ids;
mod keys;
mod layout;
mod menu;
mod paint;
mod settings_form;
mod state;
mod stats;
mod text;
mod tray;
mod update;
mod widgets;
mod window;

pub(crate) use dialog::show_hook_failed;
pub(crate) use window::{create_window, show_initial};
