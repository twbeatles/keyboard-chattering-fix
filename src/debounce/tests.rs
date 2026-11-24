use super::types::{VK_PACKET, VK_PROCESSKEY, WHEEL_REVERSE_MS};
use super::*;
use std::time::Duration;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn first_press_always_forwards() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert_eq!(d.total_presses, 1);
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn clear_key_stats_forgets_one_key_only() {
    let mut d = Debouncer::new(50);
    d.record_all_keys = true;
    // A and B chatter alike: one pass and one block each.
    for (vk, t) in [(0x41, 0), (0x42, 100)] {
        assert!(d.decide(vk, KeyEventKind::Down, ms(t)));
        assert!(d.decide(vk, KeyEventKind::Up, ms(t + 10)));
        assert!(!d.decide(vk, KeyEventKind::Down, ms(t + 20)));
    }
    assert_eq!((d.total_presses, d.blocked_presses), (4, 2));
    assert_eq!(d.last_blocked.map(|(vk, _)| vk), Some(0x42));

    d.clear_key_stats(0x41);
    assert_eq!((d.per_key_presses[0x41], d.per_key_blocked[0x41]), (0, 0));
    assert_eq!((d.per_key_presses[0x42], d.per_key_blocked[0x42]), (2, 1));
    assert_eq!((d.total_presses, d.blocked_presses), (2, 1));
    // Another key's "last blocked" survives.
    assert_eq!(d.last_blocked.map(|(vk, _)| vk), Some(0x42));
    assert!(d.gap_histogram(0x41).iter().all(|&n| n == 0));
    assert!(!d.gap_histogram(0x42).iter().all(|&n| n == 0));
    assert!(d.log_events.iter().all(|e| e.vk != 0x41));
    assert_eq!(d.log_events.iter().filter(|e| e.vk == 0x42).count(), 2);
    // Thresholds are kept.
    d.set_key_threshold(0x41, Some(120));
    d.clear_key_stats(0x41);
    assert_eq!(d.key_threshold_ms(0x41), (120, true));
}

#[test]
fn fast_bounce_is_blocked() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 20 ms after release -> inside 50 ms threshold -> blocked.
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(40)));
    assert_eq!(d.total_presses, 2);
    assert_eq!(d.blocked_presses, 1);
    assert_eq!(d.per_key_blocked[0x41], 1);
}

#[test]
fn slow_repeat_is_forwarded() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 60 ms after release -> beyond 50 ms threshold -> allowed.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(80)));
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn auto_repeat_never_blocked() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    // Consecutive Down without Up is auto-repeat.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(30)));
    assert!(d.decide(0x41, KeyEventKind::Down, ms(50)));
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn keys_are_independent() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // Different key pressed 5 ms later -> allowed.
    assert!(d.decide(0x42, KeyEventKind::Down, ms(25)));
    assert_eq!(d.blocked_presses, 0);
}

/// Korean double consonants across syllables (`썼`: Shift+T, J,
/// Shift+T): the left Shift goes down again 20 ms after it went up.
/// Blocking it turned the next `ㅆ` into `ㅅ`.
#[test]
fn shift_and_ctrl_are_not_filtered_unless_set_per_key() {
    const LSHIFT: usize = 0xA0;
    let mut d = Debouncer::new(50);
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(0)));
    assert!(d.decide(LSHIFT, KeyEventKind::Up, ms(80)));
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(100)));
    assert_eq!(d.blocked_presses, 0);
    for vk in [0x10u8, 0x11, 0xA0, 0xA1, 0xA2, 0xA3] {
        assert_eq!(d.key_threshold_ms(vk), (0, false), "vk {vk:#04x}");
    }
    // Alt and Win keep the global window; so does an explicit override.
    assert_eq!(d.key_threshold_ms(0xA4), (50, false));
    assert_eq!(d.key_threshold_ms(0x5B), (50, false));
    d.set_key_threshold(LSHIFT as u8, Some(50));
    assert!(d.decide(LSHIFT, KeyEventKind::Up, ms(150)));
    assert!(!d.decide(LSHIFT, KeyEventKind::Down, ms(170)));
}

/// `있습니다`: T with Shift (`ㅆ`), then T without it (`ㅅ`) 20 ms after
/// its release. A different character, not a bounce.
#[test]
fn a_changed_modifier_state_makes_a_new_keystroke() {
    const LSHIFT: usize = 0xA0;
    const T: usize = 0x54;
    let mut d = Debouncer::new(50);
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(0)));
    assert!(d.decide(T, KeyEventKind::Down, ms(10)));
    assert!(d.decide(T, KeyEventKind::Up, ms(60)));
    // Bounce while Shift is still held: blocked as before.
    assert!(!d.decide(T, KeyEventKind::Down, ms(65)));
    assert!(d.decide(T, KeyEventKind::Up, ms(70)));
    assert!(d.decide(LSHIFT, KeyEventKind::Up, ms(75)));
    assert!(d.decide(T, KeyEventKind::Down, ms(90)));
    assert!(d.decide(T, KeyEventKind::Up, ms(140)));
    // Same state again (no Shift): a fast re-press is chatter.
    assert!(!d.decide(T, KeyEventKind::Down, ms(150)));
    assert_eq!((d.per_key_presses[T], d.per_key_blocked[T]), (4, 2));
}

/// Fast `ㅆ` (or a capital `T`): Shift is let go before the letter.
/// A bounce of the letter after its release is still chatter, though
/// the modifier state differs from that at its press (`있` -> `있ㅅ`).
#[test]
fn a_modifier_released_before_the_key_does_not_let_its_bounce_through() {
    const LSHIFT: usize = 0xA0;
    const T: usize = 0x54;
    let mut d = Debouncer::new(50);
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(0)));
    assert!(d.decide(T, KeyEventKind::Down, ms(10)));
    assert!(d.decide(LSHIFT, KeyEventKind::Up, ms(40)));
    assert!(d.decide(T, KeyEventKind::Up, ms(60)));
    assert!(!d.decide(T, KeyEventKind::Down, ms(65)));
    assert_eq!(d.per_key_blocked[T], 1);
}

/// `쌌`: Shift+T, Shift up, K, Shift down again, T. The modifier state
/// at both presses is the same, but Shift moved in between.
#[test]
fn a_modifier_pressed_again_between_presses_makes_a_new_keystroke() {
    const LSHIFT: usize = 0xA0;
    const T: usize = 0x54;
    const K: usize = 0x4B;
    let mut d = Debouncer::new(50);
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(0)));
    assert!(d.decide(T, KeyEventKind::Down, ms(10)));
    assert!(d.decide(T, KeyEventKind::Up, ms(60)));
    assert!(d.decide(LSHIFT, KeyEventKind::Up, ms(65)));
    assert!(d.decide(K, KeyEventKind::Down, ms(70)));
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(90)));
    assert!(d.decide(T, KeyEventKind::Down, ms(100)));
    // Shift's own auto-repeat while held is no modifier change.
    assert!(d.decide(T, KeyEventKind::Up, ms(150)));
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(155)));
    assert!(!d.decide(T, KeyEventKind::Down, ms(160)));
    assert_eq!((d.per_key_presses[T], d.per_key_blocked[T]), (3, 1));
}

/// With a release delay on, a held-back Shift release reached
/// applications after the next key: `깨` came out as `꺠`.
#[test]
fn release_defer_skips_keys_that_are_never_filtered() {
    const LSHIFT: usize = 0xA0;
    let mut d = defer(30);
    assert!(d.decide(LSHIFT, KeyEventKind::Down, ms(0)));
    assert_eq!(
        d.decide_event(LSHIFT, false, KeyEventKind::Up, ms(50)),
        Decision::Pass
    );
    d.set_key_threshold(0x41, Some(0));
    assert!(d.decide(0x41, KeyEventKind::Down, ms(60)));
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Up, ms(100)),
        Decision::Pass
    );
    assert!(!d.has_pending_releases());
}

#[test]
fn per_key_threshold_overrides_global() {
    let mut d = Debouncer::new(50);
    d.set_key_threshold(0x41, Some(100));
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 70 ms gap: would pass 50 ms global, but blocked by 100 ms override.
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(90)));
    assert_eq!(d.blocked_presses, 1);
}

#[test]
fn per_key_override_can_be_cleared() {
    let mut d = Debouncer::new(50);
    d.set_key_threshold(0x41, Some(100));
    d.set_key_threshold(0x41, None);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 70 ms gap: falls back to 50 ms global, so forwards.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(90)));
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn measure_from_press_uses_last_press() {
    let mut d = Debouncer::new(50);
    d.set_measure_from(MeasureFrom::Press);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(55)));
    // Gap from press is 50 ms (at threshold -> forwards).
    assert!(d.decide(0x41, KeyEventKind::Down, ms(60)));
    assert_eq!(d.blocked_presses, 0);
    assert!(d.decide(0x41, KeyEventKind::Up, ms(65)));
    // Gap from last accepted press (60) is 20 ms -> blocked.
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(80)));
    assert_eq!(d.blocked_presses, 1);
}

#[test]
fn boundary_at_threshold_forwards() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // Exactly 50 ms after release -> forwards.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(70)));
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn last_blocked_records_gap() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(35)));
    assert_eq!(d.last_blocked, Some((0x41, ms(15))));
}

#[test]
fn min_chatter_forwards_bugged_input() {
    let mut d = Debouncer::new(50);
    d.set_min_chatter_ms(5);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 2 ms gap is below min_chatter -> treated as bugged duplicate, forwarded.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(22)));
    assert_eq!(d.blocked_presses, 0);
}

#[test]
fn min_chatter_getter_round_trips() {
    let mut d = Debouncer::new(50);
    assert_eq!(d.min_chatter_ms(), 0);
    d.set_min_chatter_ms(2000);
    assert_eq!(d.min_chatter_ms(), 1000);
}

#[test]
fn log_buffer_stays_bounded() {
    let mut d = Debouncer::new(50);
    d.record_all_keys = true;
    d.set_log_limit(10);
    for t in 0..50 {
        assert!(d.decide(0x41, KeyEventKind::Down, ms(t * 100)));
        assert!(d.decide(0x41, KeyEventKind::Up, ms(t * 100 + 50)));
    }
    // 50 recorded presses against a cap of 10: the buffer must hold
    // exactly the 10 most recent events, no more and no fewer.
    assert_eq!(d.log_events.len(), 10);
    assert_eq!(d.log_events.back().unwrap().time, ms(4900));
}

#[test]
fn cleaning_mode_blocks_until_enter() {
    let mut d = Debouncer::new(50);
    d.cleaning_mode = true;
    // Normal keys blocked
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(!d.decide(0x20, KeyEventKind::Down, ms(20)));
    assert!(d.cleaning_mode);
    // Enter key (0x0D) exits cleaning mode
    assert!(!d.decide(0x0D, KeyEventKind::Down, ms(30)));
    assert!(!d.cleaning_mode);
    // After exiting, keys work normally
    assert!(d.decide(0x41, KeyEventKind::Down, ms(40)));
}

#[test]
fn min_chatter_boundary_is_exclusive() {
    // gap == min_chatter is NOT treated as a glitch: it falls through to
    // the threshold check. Only strictly smaller gaps bypass filtering.
    let mut d = Debouncer::new(50);
    d.set_min_chatter_ms(10);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(10)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 9 ms < 10 ms min_chatter: forwarded as bugged input, not chatter.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(29)));
    assert_eq!(d.blocked_presses, 0);
    assert!(d.decide(0x41, KeyEventKind::Up, ms(30)));
    // Exactly 10 ms: not a glitch, and inside the 50 ms threshold: blocked.
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(40)));
    assert_eq!(d.blocked_presses, 1);
}

#[test]
fn log_limit_is_clamped_and_capacity_reserved() {
    let mut d = Debouncer::new(50);
    d.set_log_limit(3);
    assert_eq!(d.log_limit(), LOG_LIMIT_MIN);
    d.set_log_limit(1_000_000);
    assert_eq!(d.log_limit(), LOG_LIMIT_MAX);
    // The whole buffer is reserved up front: the hook never reallocates.
    assert!(d.log_events.capacity() >= LOG_LIMIT_MAX);
}

#[test]
fn shrinking_log_limit_keeps_newest_events() {
    let mut d = Debouncer::new(50);
    d.record_all_keys = true;
    for t in 0..20 {
        d.decide(0x41, KeyEventKind::Down, ms(t * 100));
        d.decide(0x41, KeyEventKind::Up, ms(t * 100 + 50));
    }
    d.set_log_limit(10);
    assert_eq!(d.log_events.len(), 10);
    assert_eq!(d.log_events.front().unwrap().time, ms(1000));
}

#[test]
fn log_seq_moves_on_every_log_change() {
    let mut d = Debouncer::new(50);
    let s0 = d.log_seq;
    d.decide(0x41, KeyEventKind::Down, ms(10));
    // record_all_keys off and nothing blocked: no log entry, no bump.
    assert_eq!(d.log_seq, s0);
    d.decide(0x41, KeyEventKind::Up, ms(20));
    d.decide(0x41, KeyEventKind::Down, ms(30)); // blocked -> logged
    assert_eq!(d.log_seq, s0 + 1);
    d.clear_log();
    assert_eq!(d.log_seq, s0 + 2);
    assert!(d.log_events.is_empty());
}

#[test]
fn reset_stats_zeroes_counters_and_log() {
    let mut d = Debouncer::new(50);
    d.decide(0x41, KeyEventKind::Down, ms(10));
    d.decide(0x41, KeyEventKind::Up, ms(20));
    d.decide(0x41, KeyEventKind::Down, ms(30));
    assert_eq!(d.blocked_presses, 1);
    d.reset_stats();
    assert_eq!((d.total_presses, d.blocked_presses), (0, 0));
    assert_eq!(d.per_key_blocked[0x41], 0);
    assert_eq!(d.last_blocked, None);
    assert!(d.log_events.is_empty());
    assert_eq!(d.top_chattered_key(), None);
}

#[test]
fn held_key_bounce_does_not_count_repeats_as_chatter() {
    // A held key whose contact bounces once (Up, Down) in the middle of
    // the hold: the bounce Down is chatter, but the keyboard's next
    // auto-repeat Down must resume the key, not be blocked as more
    // chatter (it used to be, until the gap since the bounce release
    // grew past the threshold).
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x57, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x57, KeyEventKind::Up, ms(200)));
    assert!(!d.decide(0x57, KeyEventKind::Down, ms(203)));
    assert!(d.decide(0x57, KeyEventKind::Down, ms(233)));
    assert!(d.decide(0x57, KeyEventKind::Down, ms(266)));
    assert_eq!(d.blocked_presses, 1);
    assert_eq!(d.per_key_blocked[0x57], 1);
}

#[test]
fn release_chatter_is_still_blocked_after_a_blocked_bounce() {
    // Down, Up, bounce Down (blocked), bounce Up, bounce Down again:
    // the Up in between ends the "held" state, so the second bounce is
    // judged as chatter again.
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(100)));
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(104)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(107)));
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(111)));
    assert_eq!(d.blocked_presses, 2);
}

#[test]
fn shared_ime_codes_always_pass_and_are_not_counted() {
    let mut d = Debouncer::new(50);
    d.record_all_keys = true;
    for vk in [VK_PACKET, VK_PROCESSKEY] {
        for t in 0..5 {
            // Different characters 2 ms apart, all reported as one code.
            assert!(d.decide(vk, KeyEventKind::Down, ms(t * 4)));
            assert!(d.decide(vk, KeyEventKind::Up, ms(t * 4 + 2)));
        }
    }
    assert_eq!((d.total_presses, d.blocked_presses), (0, 0));
    assert!(d.log_events.is_empty());
}

#[test]
fn cleaning_mode_lets_releases_through() {
    // A key held while cleaning starts must not stay "down" in apps.
    let mut d = Debouncer::new(50);
    d.cleaning_mode = true;
    assert!(d.decide(0x10, KeyEventKind::Up, ms(10)));
    assert!(!d.decide(0x10, KeyEventKind::Down, ms(20)));
}

#[test]
fn extended_and_plain_keys_sharing_a_code_are_timed_apart() {
    // Main Enter then numpad Enter 10 ms later: two physical keys.
    let mut d = Debouncer::new(50);
    let down = |d: &mut Debouncer, ext, t| d.decide_event(0x0D, ext, KeyEventKind::Down, ms(t));
    let up = |d: &mut Debouncer, ext, t| d.decide_event(0x0D, ext, KeyEventKind::Up, ms(t));
    assert_eq!(down(&mut d, false, 0), Decision::Pass);
    assert_eq!(up(&mut d, false, 30), Decision::Pass);
    assert_eq!(down(&mut d, true, 40), Decision::Pass);
    assert_eq!(up(&mut d, true, 60), Decision::Pass);
    // The same physical key bouncing is still chatter.
    assert_eq!(down(&mut d, true, 65), Decision::Block);
    // Counters stay per virtual key.
    assert_eq!((d.per_key_presses[0x0D], d.per_key_blocked[0x0D]), (3, 1));
}

fn defer(ms_: u64) -> Debouncer {
    let mut d = Debouncer::new(50);
    d.set_release_defer_ms(ms_);
    d
}

#[test]
fn release_defer_hides_a_bounce_while_held() {
    let mut d = defer(30);
    let mut out = [0u16; SLOTS];
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Down, ms(0)),
        Decision::Pass
    );
    // Mid-hold bounce: the release is held back, the bounce press is
    // swallowed, and applications never see the key go up.
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Up, ms(200)),
        Decision::Defer
    );
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Down, ms(204)),
        Decision::Block
    );
    assert!(!d.has_pending_releases());
    assert_eq!(d.blocked_presses, 1);
    // Auto-repeat continues normally.
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Down, ms(233)),
        Decision::Pass
    );
    // The real release goes out once the delay ran out.
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Up, ms(500)),
        Decision::Defer
    );
    assert_eq!(d.due_releases(ms(520), false, &mut out), 0);
    assert_eq!(d.due_releases(ms(530), false, &mut out), 1);
    assert_eq!(out[0], 0x57);
    // Measured from the real release time: 540 is 40 ms after 500.
    assert_eq!(
        d.decide_event(0x57, false, KeyEventKind::Down, ms(540)),
        Decision::Block
    );
}

#[test]
fn release_defer_flushes_a_late_release_before_the_next_press() {
    let mut d = defer(20);
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Down, ms(0)),
        Decision::Pass
    );
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Up, ms(100)),
        Decision::Defer
    );
    // The timer did not run: 300 ms later the release is still pending.
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Down, ms(300)),
        Decision::Flush { then_pass: true }
    );
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Up, ms(400)),
        Decision::Defer
    );
    // Late again, and this press is chatter after the real release.
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Down, ms(430)),
        Decision::Flush { then_pass: false }
    );
}

#[test]
fn release_defer_only_holds_releases_of_held_keys() {
    let mut d = defer(30);
    // A release with no press seen (e.g. the key was down at startup).
    assert_eq!(
        d.decide_event(0x41, false, KeyEventKind::Up, ms(10)),
        Decision::Pass
    );
    // A duplicate release while one is held back is dropped.
    assert_eq!(
        d.decide_event(0x42, false, KeyEventKind::Down, ms(20)),
        Decision::Pass
    );
    assert_eq!(
        d.decide_event(0x42, false, KeyEventKind::Up, ms(30)),
        Decision::Defer
    );
    assert_eq!(
        d.decide_event(0x42, false, KeyEventKind::Up, ms(31)),
        Decision::Block
    );
    // Turning the delay off releases what is held back at once.
    d.set_release_defer_ms(0);
    let mut out = [0u16; SLOTS];
    assert_eq!(d.due_releases(ms(32), false, &mut out), 1);
    assert_eq!(
        d.decide_event(0x42, false, KeyEventKind::Down, ms(200)),
        Decision::Pass
    );
    assert_eq!(
        d.decide_event(0x42, false, KeyEventKind::Up, ms(250)),
        Decision::Pass
    );
    assert_eq!(d.release_defer_ms(), 0);
    d.set_release_defer_ms(5000);
    assert_eq!(d.release_defer_ms(), RELEASE_DEFER_MAX_MS);
}

#[test]
fn release_defer_never_loses_a_release() {
    // Random streams with a delay on: every held-back release is either
    // cancelled by a bounce or sent, and a final flush leaves none.
    for seed in 0..100u64 {
        let mut rng = Lcg(seed);
        let mut d = defer(5 + rng.next() % 40);
        let mut out = [0u16; SLOTS];
        let mut now = 0u64;
        for _ in 0..300 {
            now += rng.next() % 50;
            if rng.next().is_multiple_of(4) {
                d.due_releases(ms(now), false, &mut out);
            }
            let vk = [0x41usize, 0x01][(rng.next() % 2) as usize];
            let kind = if rng.next().is_multiple_of(2) {
                KeyEventKind::Down
            } else {
                KeyEventKind::Up
            };
            let decision = d.decide_event(vk, false, kind, ms(now));
            if kind == KeyEventKind::Down {
                assert_ne!(decision, Decision::Defer, "seed {seed}");
            }
        }
        d.due_releases(ms(now), true, &mut out);
        assert!(!d.has_pending_releases(), "seed {seed}");
    }
}

#[test]
fn cleaning_mode_still_blocks_shared_ime_codes() {
    let mut d = Debouncer::new(50);
    d.cleaning_mode = true;
    assert!(!d.decide(VK_PACKET, KeyEventKind::Down, ms(10)));
}

#[test]
fn gap_histogram_buckets_every_measured_gap() {
    let mut d = Debouncer::new(50);
    assert!(d.decide(0x41, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(10)));
    // 3 ms after release: bucket 0, blocked.
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(13)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(20)));
    // 72 ms: bucket 14, forwarded.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(92)));
    // Auto-repeat has no gap and is not measured.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(125)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(130)));
    // 5 s: overflow bucket.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(5130)));
    let h = d.gap_histogram(0x41);
    assert_eq!((h[0], h[14], h[GAP_BUCKETS - 1]), (1, 1, 1));
    assert_eq!(h.iter().sum::<u32>(), 3);
    assert_eq!(d.gap_histogram(0x42).iter().sum::<u32>(), 0);
    d.reset_stats();
    assert_eq!(d.gap_histogram(0x41).iter().sum::<u32>(), 0);
}

/// Small deterministic generator, so the invariant test needs no extra
/// dependency and every failure is reproducible from its seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

#[test]
fn invariants_hold_for_random_event_streams() {
    let keys = [0x41usize, 0x42, 0x20, VK_PACKET];
    for seed in 0..200u64 {
        let mut rng = Lcg(seed);
        let mut d = Debouncer::new(1 + rng.next() % 120);
        d.set_min_chatter_ms(rng.next() % 6);
        d.set_measure_from(if seed % 2 == 0 {
            MeasureFrom::Release
        } else {
            MeasureFrom::Press
        });
        d.record_all_keys = seed % 3 == 0;
        d.set_log_limit(10);
        let mut now = 0u64;
        // Last event seen per key: None = never, Some(true) = Down.
        let mut last_down: [Option<bool>; 256] = [None; 256];
        for _ in 0..400 {
            now += rng.next() % 60;
            let vk = keys[(rng.next() % keys.len() as u64) as usize];
            let kind = if rng.next().is_multiple_of(2) {
                KeyEventKind::Down
            } else {
                KeyEventKind::Up
            };
            let passed = d.decide(vk, kind, ms(now));
            match kind {
                KeyEventKind::Up => assert!(passed, "seed {seed}: Up blocked"),
                KeyEventKind::Down => {
                    if last_down[vk].is_none() {
                        assert!(passed, "seed {seed}: first press blocked");
                    }
                    if last_down[vk] == Some(true) {
                        assert!(passed, "seed {seed}: auto-repeat blocked");
                    }
                    if vk == VK_PACKET {
                        assert!(passed, "seed {seed}: VK_PACKET blocked");
                    }
                }
            }
            last_down[vk] = Some(kind == KeyEventKind::Down);
        }
        assert!(d.blocked_presses <= d.total_presses, "seed {seed}");
        assert_eq!(d.per_key_presses.iter().sum::<u64>(), d.total_presses);
        assert_eq!(d.per_key_blocked.iter().sum::<u64>(), d.blocked_presses);
        assert_eq!(d.per_key_presses[VK_PACKET], 0, "seed {seed}");
        assert!(d.log_events.len() <= d.log_limit(), "seed {seed}");
        for vk in keys {
            let measured: u32 = d.gap_histogram(vk as u8).iter().sum();
            assert!(u64::from(measured) <= d.per_key_presses[vk], "seed {seed}");
        }
    }
}

#[test]
fn key_threshold_reports_override_flag() {
    let mut d = Debouncer::new(50);
    assert_eq!(d.key_threshold_ms(0x41), (50, false));
    d.set_key_threshold(0x41, Some(120));
    assert_eq!(d.key_threshold_ms(0x41), (120, true));
    assert_eq!(d.key_threshold_ms(0x42), (50, false));
}

/// `key.XX=0`: the key is never blocked, but its presses still count.
#[test]
fn zero_window_never_filters_the_key() {
    let mut d = Debouncer::new(50);
    d.set_key_threshold(0x20, Some(0));
    assert_eq!(d.key_threshold_ms(0x20), (0, true));
    assert!(d.decide(0x20, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x20, KeyEventKind::Up, ms(10)));
    assert!(d.decide(0x20, KeyEventKind::Down, ms(11)));
    assert_eq!((d.per_key_presses[0x20], d.per_key_blocked[0x20]), (2, 0));
    // Other keys keep the global window.
    assert!(d.decide(0x41, KeyEventKind::Down, ms(100)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(110)));
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(111)));
}

#[test]
fn app_profile_overrides_global_but_not_per_key() {
    let mut d = Debouncer::new(50);
    assert_eq!(d.effective_threshold(0x41), Duration::from_millis(50));
    d.set_app_threshold(Some(80));
    assert_eq!(d.effective_threshold(0x41), Duration::from_millis(80));
    // Mouse buttons and the wheel follow the profile too.
    assert_eq!(d.effective_threshold(0x01), Duration::from_millis(80));
    assert_eq!(d.key_threshold_ms(0x0A), (80, false));
    // A per-key override still wins over the profile.
    d.set_key_threshold(0x41, Some(120));
    assert_eq!(d.key_threshold_ms(0x41), (120, true));
    assert_eq!(d.effective_threshold(0x41), Duration::from_millis(120));
    // Zero never filters (but still counts presses).
    d.set_app_threshold(Some(0));
    assert!(d.decide(0x20, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x20, KeyEventKind::Up, ms(10)));
    assert!(d.decide(0x20, KeyEventKind::Down, ms(11)));
    assert_eq!((d.per_key_presses[0x20], d.per_key_blocked[0x20]), (2, 0));
    // Clearing restores the global (and wheel) windows.
    d.set_key_threshold(0x41, None);
    d.set_app_threshold(None);
    assert_eq!(d.effective_threshold(0x41), Duration::from_millis(50));
    assert_eq!(d.key_threshold_ms(0x0A), (WHEEL_REVERSE_MS, false));
}

#[test]
fn mouse_buttons_use_their_own_window_unless_overridden() {
    let mut d = Debouncer::new(80);
    assert_eq!(
        d.key_threshold_ms(0x01),
        (MOUSE_THRESHOLD_DEFAULT_MS, false)
    );
    // VK_CANCEL (0x03) is a keyboard key: global window.
    assert_eq!(d.key_threshold_ms(0x03), (80, false));
    d.set_mouse_threshold_ms(0);
    assert_eq!(d.mouse_threshold_ms(), 1, "clamped like the global window");
    d.set_mouse_threshold_ms(40);
    // A quick double-click (45 ms release-to-press) passes, a 20 ms
    // bounce does not, while the keyboard keeps its 80 ms.
    assert!(d.decide(0x01, KeyEventKind::Down, ms(0)));
    assert!(d.decide(0x01, KeyEventKind::Up, ms(60)));
    assert!(d.decide(0x01, KeyEventKind::Down, ms(105)));
    assert!(d.decide(0x01, KeyEventKind::Up, ms(160)));
    assert!(!d.decide(0x01, KeyEventKind::Down, ms(180)));
    assert!(d.decide(0x41, KeyEventKind::Down, ms(200)));
    assert!(d.decide(0x41, KeyEventKind::Up, ms(260)));
    assert!(!d.decide(0x41, KeyEventKind::Down, ms(305)));
    // A per-button override wins over the mouse window.
    d.set_key_threshold(0x02, Some(0));
    assert_eq!(d.key_threshold_ms(0x02), (0, true));
    assert_eq!(d.key_threshold_ms(0x04), (40, false));
}

#[test]
fn wheel_same_direction_always_passes() {
    use super::WheelAxis::Vertical as V;
    let mut d = Debouncer::new(50);
    assert!(d.decide_wheel(120, ms(0), V));
    assert!(d.decide_wheel(120, ms(5), V));
    assert!(d.decide_wheel(240, ms(10), V));
    assert_eq!(d.blocked_presses, 0);
    assert_eq!(d.per_key_presses[WHEEL_VK], 3);
}

#[test]
fn wheel_fast_reversal_is_blocked_but_next_forward_passes() {
    use super::WheelAxis::Vertical as V;
    let mut d = Debouncer::new(50);
    assert!(d.decide_wheel(120, ms(0), V));
    // 10 ms later the encoder briefly reports the opposite direction.
    assert!(!d.decide_wheel(-120, ms(10), V));
    assert_eq!(d.per_key_blocked[WHEEL_VK], 1);
    assert_eq!(d.last_blocked, Some((WHEEL_VK as u8, ms(10))));
    // The blocked tick did not move the direction: scrolling on passes.
    assert!(d.decide_wheel(120, ms(20), V));
    assert_eq!(d.blocked_presses, 1);
    // A slow intentional direction change passes.
    assert!(d.decide_wheel(-120, ms(500), V));
    assert_eq!(d.blocked_presses, 1);
}

#[test]
fn wheel_window_boundary_and_zero_override() {
    use super::WheelAxis::Vertical as V;
    let mut d = Debouncer::new(50);
    assert!(d.decide_wheel(120, ms(0), V));
    // Exactly the window: forwards.
    assert!(d.decide_wheel(-120, ms(WHEEL_REVERSE_MS), V));
    // Per-key override wins, and 0 never filters.
    d.set_key_threshold(WHEEL_VK as u8, Some(200));
    assert!(!d.decide_wheel(120, ms(WHEEL_REVERSE_MS + 10), V));
    d.set_key_threshold(WHEEL_VK as u8, Some(0));
    assert!(d.decide_wheel(-120, ms(WHEEL_REVERSE_MS + 20), V));
    assert_eq!(d.blocked_presses, 1);
    // Clearing the wheel's stats forgets its direction too.
    d.clear_key_stats(WHEEL_VK as u8);
    assert!(d.decide_wheel(-120, ms(WHEEL_REVERSE_MS + 30), V));
    d.reset_stats();
    assert!(d.decide_wheel(120, ms(10_000), V));
    assert_eq!((d.total_presses, d.blocked_presses), (1, 0));
}

#[test]
fn wheel_axes_are_tracked_independently() {
    use super::WheelAxis::{Horizontal as H, Vertical as V};
    let mut d = Debouncer::new(50);
    assert!(d.decide_wheel(120, ms(0), V));
    // A horizontal tick right after a vertical one is a new axis, never a
    // reversal of the vertical scroll.
    assert!(d.decide_wheel(-120, ms(10), H));
    assert_eq!(d.blocked_presses, 0);
    // Reversals are judged per axis: both of these are chatter-fast.
    assert!(!d.decide_wheel(-120, ms(20), V));
    assert!(!d.decide_wheel(120, ms(30), H));
    assert_eq!(d.blocked_presses, 2);
}

/// The Keys tab shows `key_threshold_ms`; it must agree with the window
/// `decide_wheel` actually filters with (ISSUE-001).
#[test]
fn wheel_threshold_display_matches_the_filter_window() {
    use super::types::WHEEL_VK;
    let mut d = Debouncer::new(50);
    assert_eq!(
        d.key_threshold_ms(WHEEL_VK as u8),
        (WHEEL_REVERSE_MS, false)
    );
    d.set_key_threshold(WHEEL_VK as u8, Some(200));
    assert_eq!(d.key_threshold_ms(WHEEL_VK as u8), (200, true));
    d.set_key_threshold(WHEEL_VK as u8, Some(0));
    assert_eq!(d.key_threshold_ms(WHEEL_VK as u8), (0, true));
}

#[test]
fn measured_gaps_can_be_restored_and_cleared() {
    let mut d = Debouncer::new(50);
    let mut saved: Box<[GapHistogram; 256]> = vec![[0; GAP_BUCKETS]; 256]
        .into_boxed_slice()
        .try_into()
        .unwrap();
    saved[0x41][1] = 7;
    d.set_gap_histograms(&saved);
    assert_eq!(d.gap_histogram(0x41)[1], 7);
    d.decide(0x41, KeyEventKind::Down, ms(0));
    d.decide(0x41, KeyEventKind::Up, ms(10));
    d.decide(0x41, KeyEventKind::Down, ms(16));
    assert_eq!(d.gap_histogram(0x41)[1], 8, "new gaps add to restored ones");
    d.clear_gap_histograms();
    assert_eq!(d.gap_histograms().iter().flatten().sum::<u32>(), 0);
    assert_eq!(d.per_key_presses[0x41], 2, "counters are kept");
}
