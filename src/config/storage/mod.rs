//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! Crash-safe atomic writes live in `io`; parsing in `parse`, rendering in
//! `serialize`, locations in `paths`, validation in `guard`, decoding in `encoding`.
mod encoding;
mod guard;
mod io;
mod parse;
mod paths;
mod serialize;
#[cfg(test)]
mod tests;
pub use io::{load, load_from, save, try_load};
pub use paths::config_path;
