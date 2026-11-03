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
//! - `input`: low-level keyboard hook (`hook`:
//!   `queue`/`procs`/`send`/`control`) and audio feedback (`sound`)
//! - `config`: settings model (`model`), atomic persistence (`storage`:
//!   `paths`/`encoding`/`guard`/`parse`/`serialize`/`io`), startup entry
//!   (`autostart`: `key`/`task`/`consts`/`marker`/`run`), sound path (`sound`)
//! - `debounce`: time-based chatter filter core
//!   (`types`/`filter`/`thresholds`/`stats`)
//! - `recommend`: per-key threshold advice from measured gaps (`advice`)
//! - `i18n`: language selection (`language`) and localizable strings
//!   (`strings`: `schema`/`ko`/`en`/`api`)
//! - `ui`: native Win32 presentation — `window` (`nonce`/`create`/`handlers`/
//!   `dark_menu`/`session`/`proc`), `stats` (`model`/`render`/`tick`/`update`),
//!   `history` (`view`/`clock`/`rows`/`csv`/`table`), `keys_page`
//!   (`model`/`table`/`actions`), `layout` (`page`/`kinds`/`specs`/`query`/
//!   `state`), `settings_form` (`fields`/`persist`/`sound`/`save`), plus tray,
//!   paint, commands, update flow
//! - `updater`: signed release-update client over GitHub Releases — `staging`
//!   (`guard`/`helper`/`stage`/`launch`) among `error`/`model`/`version`/
//!   `canonical`/`manifest`/`http`

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
