//! KeyboardChatteringFix: lightweight keyboard anti-chatter filter.
//!
//! A single native executable. A low-level keyboard hook drops a repeated
//! press of the same key when it arrives inside the debounce threshold
//! after its release. Auto-repeat while holding a key always passes.
//!
//! Module map (single responsibility per folder):
//!
//! - `app`: startup orchestration (`bootstrap`), terminal CLI (`cli`),
//!   single-instance guard (`instance`), process-wide state (`state`)
//! - `input`: low-level keyboard hook (`hook`) and audio feedback (`sound`)
//! - `config`: settings model, atomic persistence, startup entry, sound path
//! - `debounce`: time-based chatter filter core
//! - `recommend`: per-key threshold advice from measured gaps
//! - `i18n`: language selection and localizable strings
//! - `ui`: native Win32 presentation (window, tray, paint, forms, update flow)
//! - `updater`: signed release-update client over GitHub Releases

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod config;
mod debounce;
mod i18n;
mod input;
mod recommend;
mod ui;
mod updater;

fn main() {
    if app::cli::handle_cli() {
        return;
    }
    app::bootstrap::run(config::load());
}
