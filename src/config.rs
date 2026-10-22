//! Configuration: settings model, atomic persistence, Windows startup,
//! and sound-path validation.
//!
//! - `model`: the `Settings` struct and defaults
//! - `storage`: atomic `settings.ini` load/save with `.bak` fallback
//! - `autostart`: `HKCU\...\Run` integration and the elevated sign-in task
//! - `sound`: one-time validation of the custom notification sound

/// Application version. Single source of truth for the update pipeline:
/// must match `[package] version` in `Cargo.toml` and the `v{VERSION}`
/// GitHub Release tag. `tests/version_sync.rs` enforces this.
pub const VERSION: &str = "0.3.4";
/// Project page opened from Help > Homepage.
pub const HOMEPAGE_URL: &str = "https://github.com/twbeatles/keyboard-chattering-fix";
/// Ed25519 public key (base64, 32 bytes) that verifies `update.json`.
/// The matching private key lives only as the `KCFIX_UPDATE_PRIVATE_KEY_B64`
/// GitHub Actions secret. See `docs/UPDATE.md`.
pub const UPDATE_PUBLIC_KEY_B64: &str = "YFczqFoqyNDWEXJrbbgLvJoIOMrNT3q0pigHPnYI0hU=";

mod autostart;
mod model;
mod sound;
mod storage;

pub use autostart::{
    AUTOSTART_ARG, TaskError, autostart_enabled, clear_elevated_start_marker,
    elevated_start_expected, exe_in_protected_location, mark_elevated_start, refresh_elevated_task,
    set_elevated_task, sync_autostart, upgrade_autostart_entry,
};
pub use model::Settings;
pub use sound::{ValidatedSound, resolve_sound_file};
pub use storage::{config_path, load, load_from, save, try_load};
