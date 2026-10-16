//! Configuration: settings model, atomic persistence, Windows startup,
//! and sound-path validation.
//!
//! - `model`: the `Settings` struct and defaults
//! - `storage`: atomic `settings.ini` load/save with `.bak` fallback
//! - `autostart`: `HKCU\...\Run` integration
//! - `sound`: one-time validation of the custom notification sound

/// Application version. Single source of truth for the update pipeline:
/// must match `[package] version` in `Cargo.toml` and the `v{VERSION}`
/// GitHub Release tag. `tests/version_sync.rs` enforces this.
pub const VERSION: &str = "0.2.0";
/// Ed25519 public key (base64, 32 bytes) that verifies `update.json`.
/// The matching private key lives only as the `KCFIX_UPDATE_PRIVATE_KEY_B64`
/// GitHub Actions secret. See `docs/UPDATE.md`.
pub const UPDATE_PUBLIC_KEY_B64: &str = "YFczqFoqyNDWEXJrbbgLvJoIOMrNT3q0pigHPnYI0hU=";

mod autostart;
mod model;
mod sound;
mod storage;

pub use autostart::{autostart_enabled, set_autostart};
pub use model::Settings;
pub use sound::{resolve_sound_file, ValidatedSound};
pub use storage::{load, save};
