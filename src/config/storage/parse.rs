//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use std::collections::HashMap;

use super::super::model::Settings;
use crate::{
    debounce::{
        GAP_BUCKETS, GapHistogram, LOG_LIMIT_MAX, LOG_LIMIT_MIN, MeasureFrom, RELEASE_DEFER_MAX_MS,
    },
    i18n::LanguagePref,
};

/// Upper bound of `iCleaningTimeoutMin`.
pub const CLEANING_TIMEOUT_MAX_MIN: u64 = 120;

/// The value of a `key=value` line without a trailing comment: `#` or `;`
/// after whitespace starts one (`bPlaySound=0   # comment`), and so does a
/// value that is nothing but a comment. The sound path keeps everything
/// after its first character, since a file name may contain " #".
pub(super) fn strip_inline_comment<'a>(key: &str, value: &'a str) -> &'a str {
    let value = value.trim();
    if value.starts_with(['#', ';']) {
        return "";
    }
    if key == "lpSoundFile" {
        return value;
    }
    let cut = value
        .char_indices()
        .find(|&(i, c)| {
            matches!(c, '#' | ';')
                && value[..i].ends_with(char::is_whitespace)
                // `;` also separates excluded apps (`a.exe ; b.exe`).
                && !(c == ';' && key == "sAutoDisableApps")
        })
        .map_or(value.len(), |(i, _)| i);
    value[..cut].trim_end()
}

pub(super) fn parse(text: &str) -> Settings {
    let mut s = Settings::default();
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';', '[']) {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            map.insert(k.to_string(), strip_inline_comment(k, v).to_string());
        }
    }
    if let Some(v) = map.get("iThreshold")
        && let Ok(n) = v.parse::<u64>()
    {
        s.threshold_ms = n.clamp(1, 1000);
    }
    if let Some(v) = map
        .get("bFilterEnabled")
        .or_else(|| map.get("bFilterStatus"))
    {
        s.enabled = v != "0";
    }
    if let Some(v) = map.get("bPlaySound") {
        s.play_sound = v != "0";
    }
    if let Some(v) = map.get("lpSoundFile") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            s.sound_file = Some(trimmed.to_string());
        }
    }
    if let Some(v) = map
        .get("bMinimizeOnLaunch")
        .or_else(|| map.get("bMinimizeWhenLaunch"))
    {
        s.minimize_on_launch = v != "0";
    }
    if let Some(v) = map.get("bCloseToTray") {
        s.close_to_tray = v != "0";
    }
    if let Some(v) = map.get("bRecordAllKeypress") {
        s.record_all_keys = v != "0";
    }
    if let Some(v) = map.get("iLogLimit")
        && let Ok(n) = v.parse::<usize>()
    {
        s.log_limit = n.clamp(LOG_LIMIT_MIN, LOG_LIMIT_MAX);
    }
    if let Some(v) = map.get("sLanguage") {
        s.language = LanguagePref::from_setting(v);
    }
    if let Some(v) = map.get("sMeasureFrom") {
        s.measure_from = measure_from(v);
    }
    if let Some(v) = map.get("iMinChatterMs")
        && let Ok(n) = v.parse::<u64>()
    {
        s.min_chatter_ms = n.min(1000);
    }
    if let Some(v) = map.get("bExcludeInjected") {
        s.exclude_injected = v != "0";
    }
    if let Some(v) = map.get("bFilterMouse") {
        s.filter_mouse = v != "0";
    }
    if let Some(v) = map.get("iMouseThreshold")
        && let Ok(n) = v.parse::<u64>()
    {
        s.mouse_threshold_ms = n.clamp(1, 1000);
    }
    if let Some(v) = map.get("iReleaseDeferMs")
        && let Ok(n) = v.parse::<u64>()
    {
        s.release_defer_ms = n.min(RELEASE_DEFER_MAX_MS);
    }
    if let Some(v) = map.get("sAutoDisableApps") {
        s.auto_disable_apps = v.clone();
    }
    if let Some(v) = map.get("bAutoDisableFullscreen") {
        s.auto_disable_fullscreen = v != "0";
    }
    if let Some(v) = map.get("sHotkeyToggle") {
        s.hotkey_toggle = v.clone();
    }
    if let Some(v) = map.get("sHotkeyCleaning") {
        s.hotkey_cleaning = v.clone();
    }
    if let Some(v) = map.get("iCleaningTimeoutMin")
        && let Ok(n) = v.parse::<u64>()
    {
        s.cleaning_timeout_min = n.min(CLEANING_TIMEOUT_MAX_MIN);
    }
    if let Some(v) = map.get("iTotalPresses")
        && let Ok(n) = v.parse::<u64>()
    {
        s.total_presses = n;
    }
    if let Some(v) = map.get("iBlockedPresses")
        && let Ok(n) = v.parse::<u64>()
    {
        s.blocked_presses = n;
    }
    // Per-key cumulative counters: stat.<hex vk>=<presses>,<blocked>.
    // Only non-zero entries are written, so absence means zero.
    for (k, v) in &map {
        if let Some(hex) = k.strip_prefix("stat.")
            && let Ok(vk) = u8::from_str_radix(hex, 16)
            && let Some((presses, blocked)) = v.split_once(',')
            && let (Ok(p), Ok(b)) = (presses.parse::<u64>(), blocked.parse::<u64>())
        {
            s.per_key_presses[vk as usize] = p;
            s.per_key_blocked[vk as usize] = b;
        }
    }
    // Per-key overrides: key.<2-digit hex virtual-key code>=<ms>.
    for (k, v) in &map {
        if let Some(hex) = k.strip_prefix("key.")
            && let (Ok(vk), Ok(ms)) = (u8::from_str_radix(hex, 16), v.parse::<u64>())
        {
            // 0 = never filter this key.
            s.per_key_ms[vk as usize] = Some(ms.min(1000));
        }
    }
    // Measured gaps: hist.<hex vk>=<bucket>:<count>,... (non-zero buckets
    // only). They only mean something under the baseline they were measured
    // from, so a file whose sMeasureFrom was changed by hand drops them.
    let baseline = map
        .get("sHistMeasure")
        .map_or(s.measure_from, |v| measure_from(v));
    if baseline == s.measure_from {
        for (k, v) in &map {
            if let Some(hex) = k.strip_prefix("hist.")
                && let Ok(vk) = u8::from_str_radix(hex, 16)
            {
                parse_histogram(v, &mut s.gap_hist[vk as usize]);
            }
        }
    }
    s
}

pub(super) fn measure_from(v: &str) -> MeasureFrom {
    if v.eq_ignore_ascii_case("press") {
        MeasureFrom::Press
    } else {
        MeasureFrom::Release
    }
}

/// `0:3,12:5,40:312` into `h`. A malformed pair or a bucket out of range is
/// skipped on its own, like any other unreadable value.
pub(super) fn parse_histogram(v: &str, h: &mut GapHistogram) {
    for pair in v.split(',') {
        if let Some((bucket, count)) = pair.split_once(':')
            && let (Ok(bucket), Ok(count)) =
                (bucket.trim().parse::<usize>(), count.trim().parse::<u32>())
            && bucket < GAP_BUCKETS
        {
            h[bucket] = count;
        }
    }
}
