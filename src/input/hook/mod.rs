//! Low-level keyboard/mouse hook and its audio feedback.
//!
//! Queueing lives in `queue`, the hook procedures in `procs`, event
//! injection in `send`, and thread lifecycle in `control`.
mod control;
mod procs;
mod queue;
mod send;
#[cfg(test)]
mod tests;
pub(crate) use control::{
    flush_deferred_releases, restart, set_mouse_filter, set_wheel_filter, start, stop,
};
pub(crate) use queue::{HOOK_EVENTS, INJECT_MARK, InjectQueue, PROBE_VK, PROBES_SEEN};
