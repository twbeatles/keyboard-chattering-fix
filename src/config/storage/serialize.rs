//! Atomic settings persistence (`settings.ini` + `.bak` + `.tmp`).
//!
//! A crash mid-write can never leave a torn main file: the new generation
//! is staged to `settings.ini.tmp`, the previous generation is kept as
//! `settings.ini.bak`, and only then renamed over the main file. A missing
//! or empty main file falls back to the backup (ISSUE-004).

use super::super::model::Settings;
use crate::debounce::MeasureFrom;

/// A free-text value on one INI line (a pasted newline would split it).
pub(super) fn one_line(v: &str) -> String {
    v.replace(['\r', '\n'], " ").trim().to_string()
}

pub(super) fn measure_from_value(m: MeasureFrom) -> &'static str {
    match m {
        MeasureFrom::Press => "press",
        MeasureFrom::Release => "release",
    }
}

/// `hist.XX` lines for every key with measured gaps (empty when none).
pub(super) fn histogram_lines(s: &Settings) -> String {
    let mut text = String::new();
    for (vk, h) in s.gap_hist.iter().enumerate() {
        let pairs: Vec<String> = h
            .iter()
            .enumerate()
            .filter(|&(_, &n)| n > 0)
            .map(|(bucket, n)| format!("{bucket}:{n}"))
            .collect();
        if !pairs.is_empty() {
            text.push_str(&format!("hist.{vk:02X}={}\n", pairs.join(",")));
        }
    }
    text
}

/// `day.YYYYMMDD` lines for days with input (`vk:presses:blocked`, only
/// keys with activity; empty when the filter never ran that long).
pub(super) fn day_lines(s: &Settings) -> String {
    let mut text = String::new();
    for entry in &s.day_history {
        let pairs: Vec<String> = entry
            .presses
            .iter()
            .zip(entry.blocked.iter())
            .enumerate()
            .filter(|&(_, (&p, &b))| p > 0 || b > 0)
            .map(|(vk, (&p, &b))| format!("{vk:02X}:{p}:{b}"))
            .collect();
        if !pairs.is_empty() {
            text.push_str(&format!("day.{:08}={}\n", entry.date, pairs.join(",")));
        }
    }
    text
}

/// `alert.YYYYMMDD` lines: keys already warned about that day (empty when
/// no deterioration was ever seen).
pub(super) fn alert_lines(s: &Settings) -> String {
    let mut text = String::new();
    for (date, vks) in &s.alerted {
        if !vks.is_empty() {
            let list: Vec<String> = vks.iter().map(|vk| format!("{vk:02X}")).collect();
            text.push_str(&format!("alert.{date:08}={}\n", list.join(",")));
        }
    }
    text
}
