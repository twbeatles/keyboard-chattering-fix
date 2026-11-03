//! Main window: control construction, painting, page switching, message
//! dispatch, and teardown.
//!
//! The procedure only routes messages; each domain lives in its own module:
//! `layout` describes the controls, `widgets` draws the owner-drawn ones,
//! `stats`/`history` refresh content, `settings_form` saves, `commands`
//! implements menu/tray/button actions, and `update` drives the
//! release-update flow.
//!
//! One responsibility per submodule (SRP):
//!
//! - `nonce`: the process-global `WM_UPDATE_EXIT` authentication nonce
//! - `create`: control construction, visibility, window creation/showing
//! - `handlers`: `WM_*` message handlers (create/paint/command/destroy)
//! - `dark_menu`: dark owner-drawn menu-bar hooks
//! - `session`: `WM_QUERYENDSESSION`/`WM_ENDSESSION` reply policy
//! - `proc`: the `wndproc` message dispatcher

mod create;
mod dark_menu;
mod handlers;
mod nonce;
mod proc;
mod session;

#[cfg(test)]
mod tests;

pub(crate) use create::{apply_visibility, create_window, show_initial};
pub(crate) use nonce::update_exit_nonce;
