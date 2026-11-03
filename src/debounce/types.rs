//! Shared value types for the chatter filter: events, thresholds, key helpers.
use std::time::Duration;

/// Press/release event for one virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyEventKind {
    Down,
    Up,
}

/// Reference point for measuring the chatter gap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeasureFrom {
    /// Gap since the last *accepted press* (like the original kcfix).
    Press,
    /// Gap since the last *release* (default; immune to slow releases).
    Release,
}

/// A recorded key event, shown in the window's History tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogEvent {
    pub time: Duration,
    pub vk: u8,
    pub blocked: bool,
    pub gap_ms: Option<u64>,
}

/// `VK_PACKET`: Unicode text from IMEs, on-screen keyboards and remote
/// tools. Every character shares this code, so two different characters
/// typed quickly would look like one key chattering.
pub const VK_PACKET: usize = 0xE7;
/// `VK_PROCESSKEY`: a key the IME consumed; distinct physical keys share it.
pub const VK_PROCESSKEY: usize = 0xE5;

/// Width of one gap histogram bucket.
pub const GAP_BUCKET_MS: u64 = 5;
/// 40 buckets of 5 ms (0–199 ms) plus one for every gap of 200 ms or more.
pub const GAP_BUCKETS: usize = 41;
/// Count of measured gaps per bucket for one key (see [`Debouncer::gap_histogram`]).
pub type GapHistogram = [u32; GAP_BUCKETS];

/// Timing state is tracked per physical key: the virtual-key code plus the
/// extended flag (main Enter vs. numpad Enter, arrow cluster vs. numpad
/// arrows share a code). Counters and thresholds stay per virtual key.
pub const SLOTS: usize = 512;

/// Timing slot of a key: `vk`, or `vk + 256` for the extended variant.
pub fn slot_of(vk: usize, extended: bool) -> usize {
    (vk & 0xFF) | (usize::from(extended) << 8)
}

/// Default block window of keys (`iThreshold`). Bounces measured from
/// the release cluster under about 25 ms, while a fast real re-press of
/// the same key — `안녕` (S S), `빨리` (F F), `ㅋㅋ`, `ll` — starts around
/// 35 ms, so a wider default swallowed such letters.
pub const THRESHOLD_DEFAULT_MS: u64 = 35;

/// Default block window of mouse buttons (`iMouseThreshold`): below the
/// release-to-press gap of a quick double-click, above contact bounce.
pub const MOUSE_THRESHOLD_DEFAULT_MS: u64 = 30;

/// Virtual-key codes the mouse hook uses for buttons (left, right, middle,
/// X1, X2). 0x03 is `VK_CANCEL`, a keyboard key.
pub fn is_mouse_button(vk: usize) -> bool {
    matches!(vk, 0x01 | 0x02 | 0x04..=0x06)
}

/// Shift and Ctrl (generic, left, right): never filtered unless the key has
/// its own `key.XX`. Alone they type nothing, so a doubled press is
/// harmless, while blocking a quick genuine re-press changes the next key:
/// Shift+T (`ㅆ`) comes out as `ㅅ`, Shift+R (`ㄲ`) as `ㄱ`, and so on.
pub fn is_unfiltered_modifier(vk: usize) -> bool {
    matches!(vk, 0x10 | 0x11 | 0xA0..=0xA3)
}

/// Bit of a modifier key in [`Debouncer`]'s held-modifier mask (left and
/// right Shift, Ctrl, Alt, Win); 0 for every other key.
pub(crate) fn modifier_bit(vk: usize) -> u8 {
    match vk {
        0xA0..=0xA5 => 1 << (vk - 0xA0),
        0x10 => 1,
        0x11 => 1 << 2,
        0x12 => 1 << 4,
        0x5B => 1 << 6,
        0x5C => 1 << 7,
        _ => 0,
    }
}

/// Longest allowed release delay (`iReleaseDeferMs`).
pub const RELEASE_DEFER_MAX_MS: u64 = 100;

/// What the hook does with one event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Forward the event.
    Pass,
    /// Drop the event.
    Block,
    /// Drop this release for now: it is sent later by
    /// [`Debouncer::due_releases`], or cancelled when the key bounces back
    /// down in time (release delay, `iReleaseDeferMs`).
    Defer,
    /// A held-back release is overdue (its timer fired late): drop this
    /// event, send the release first, then send this press again when
    /// `then_pass`.
    Flush { then_pass: bool },
}
