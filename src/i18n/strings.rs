//! Localizable UI strings (Korean / English).
//!
//! Wording rule: describe what the user sees ("a key typed twice"), not the
//! mechanism ("debounce threshold"). Placeholders are positional `{}`.

use super::language::Language;

pub struct Strings {
    // Window chrome
    pub title: &'static str,
    pub subtitle: &'static str,
    pub status_on: &'static str,
    pub status_off: &'static str,
    pub tab_home: &'static str,
    pub tab_history: &'static str,
    pub tab_settings: &'static str,
    pub tab_advanced: &'static str,
    pub footer_tray: &'static str,
    pub footer_exit: &'static str,

    // Home
    pub enable_filter: &'static str,
    pub home_desc: &'static str,
    pub card_stats: &'static str,
    pub stat_total: &'static str,
    pub stat_blocked: &'static str,
    pub stat_ratio: &'static str,
    pub last_blocked_fmt: &'static str,
    pub last_blocked_none: &'static str,
    pub top_chattered_fmt: &'static str,
    pub top_chattered_none: &'static str,
    pub card_tools: &'static str,
    pub clean_mode_btn: &'static str,
    pub reset_stats_btn: &'static str,
    pub open_cpl_btn: &'static str,
    pub home_tip: &'static str,

    // Cleaning mode overlay
    pub clean_title: &'static str,
    pub clean_body: &'static str,
    pub clean_end_btn: &'static str,

    // History
    pub history_views: &'static [&'static str],
    pub history_search_cue: &'static str,
    pub history_clear_btn: &'static str,
    pub col_time: &'static str,
    pub col_key: &'static str,
    pub col_result: &'static str,
    pub col_gap: &'static str,
    pub col_presses: &'static str,
    pub col_blocked: &'static str,
    pub col_rate: &'static str,
    pub col_threshold: &'static str,
    pub result_blocked: &'static str,
    pub result_passed: &'static str,
    pub threshold_custom_fmt: &'static str,
    pub history_count_fmt: &'static str,
    pub history_note_blocked: &'static str,
    pub history_note_all_off: &'static str,
    pub history_note_all_on: &'static str,
    pub history_note_keys: &'static str,

    // Settings
    pub card_threshold: &'static str,
    pub threshold_label: &'static str,
    pub unit_ms: &'static str,
    pub threshold_hint: &'static str,
    pub card_sound: &'static str,
    pub play_sound: &'static str,
    pub sound_test: &'static str,
    pub sound_file_cue: &'static str,
    pub sound_browse: &'static str,
    pub sound_filter: &'static str,
    pub card_app: &'static str,
    pub autostart: &'static str,
    pub minimize_on_launch: &'static str,
    pub close_to_tray: &'static str,
    pub language_label: &'static str,
    pub save: &'static str,
    pub status_saved: &'static str,
    pub status_dirty: &'static str,
    pub save_failed: &'static str,

    // Advanced
    pub card_detect: &'static str,
    pub measure_from_label: &'static str,
    pub measure_options: &'static [&'static str],
    pub min_chatter_label: &'static str,
    pub min_chatter_unit: &'static str,
    pub min_chatter_hint: &'static str,
    pub exclude_injected: &'static str,
    pub card_log: &'static str,
    pub record_all_keys: &'static str,
    pub log_limit_label: &'static str,
    pub log_limit_unit: &'static str,
    pub log_note: &'static str,

    // Menu bar
    pub menu_file: &'static str,
    pub menu_hide: &'static str,
    pub menu_tools: &'static str,
    pub menu_protect: &'static str,
    pub menu_clear_log: &'static str,
    pub menu_open_config: &'static str,
    pub menu_help: &'static str,
    pub menu_homepage: &'static str,
    pub menu_about: &'static str,
    pub about_fmt: &'static str,

    // Tray
    pub tray_tip_on: &'static str,
    pub tray_tip_off: &'static str,
    pub tray_balloon: &'static str,
    pub menu_show: &'static str,
    pub menu_disable: &'static str,
    pub menu_enable: &'static str,
    pub menu_cleaning: &'static str,
    pub menu_update: &'static str,
    pub menu_exit: &'static str,

    // Dialogs
    pub already_running: &'static str,
    pub reset_confirm: &'static str,
    pub update_title: &'static str,
    pub update_failed_title: &'static str,
    pub update_latest: &'static str,
    pub update_available: &'static str,
    pub hook_failed: &'static str,
    pub autostart_failed: &'static str,
    pub update_busy: &'static str,
}

/// Language names are shown in their own language, so they never change.
pub const LANGUAGE_NAMES: &[&str] = &["한국어", "English"];

static KO: Strings = Strings {
    title: "키보드 채터링 방지",
    subtitle: "한 번 누른 키가 두 번 입력되는 현상을 막아줍니다",
    status_on: "● 보호 켜짐",
    status_off: "○ 보호 꺼짐",
    tab_home: "홈",
    tab_history: "기록",
    tab_settings: "설정",
    tab_advanced: "고급",
    footer_tray: "v{} · 창을 닫아도 트레이에서 계속 보호합니다",
    footer_exit: "v{} · 창을 닫으면 프로그램이 종료됩니다",

    enable_filter: "채터링 막기",
    home_desc: "같은 키가 눈 깜짝할 사이에 다시 입력되면 고장 신호(채터링)로 보고 막습니다. 평소 타자에는 영향이 없습니다.",
    card_stats: "입력 통계",
    stat_total: "전체 입력",
    stat_blocked: "막은 입력",
    stat_ratio: "막은 비율",
    last_blocked_fmt: "마지막으로 막은 키: {} ({}ms 만에 다시 눌림)",
    last_blocked_none: "마지막으로 막은 키: 아직 없음",
    top_chattered_fmt: "가장 자주 튄 키: {} ({}회 막음, 이 키 입력의 {}%)",
    top_chattered_none: "가장 자주 튄 키: 아직 없음",
    card_tools: "도구",
    clean_mode_btn: "키보드 청소 모드",
    reset_stats_btn: "통계 초기화",
    open_cpl_btn: "Windows 키보드 설정",
    home_tip: "팁: 작업 표시줄의 트레이 아이콘을 우클릭하면 보호를 잠시 끄거나 청소 모드를 바로 시작할 수 있어요.",

    clean_title: "키보드 청소 모드",
    clean_body: "지금은 모든 키 입력을 막고 있어요. 안심하고 키보드를 닦으세요.\n끝내려면 Enter 키를 누르거나 아래 버튼을 클릭하세요.",
    clean_end_btn: "청소 끝내기",

    history_views: &["막은 입력만", "모든 입력", "키별 통계"],
    history_search_cue: "키 이름으로 찾기 (예: Space)",
    history_clear_btn: "기록 지우기",
    col_time: "시간",
    col_key: "키",
    col_result: "결과",
    col_gap: "간격",
    col_presses: "입력",
    col_blocked: "막음",
    col_rate: "막은 비율",
    col_threshold: "차단 기준",
    result_blocked: "막음",
    result_passed: "통과",
    threshold_custom_fmt: "{} ms (개별)",
    history_count_fmt: "{}개 표시",
    history_note_blocked: "막은 입력이 생길 때마다 여기에 기록됩니다. 기록은 프로그램을 끄면 사라집니다.",
    history_note_all_off: "지금은 막은 입력만 기록 중이에요. 모든 입력을 보려면 [고급] 탭에서 '모든 키 입력 기록'을 켜세요.",
    history_note_all_on: "모든 키 입력을 기록 중입니다. 기록은 프로그램을 끄면 사라집니다.",
    history_note_keys: "키별 누적 통계는 저장되어 다시 실행해도 유지됩니다.",

    card_threshold: "차단 기준",
    threshold_label: "차단 기준 시간",
    unit_ms: "ms",
    threshold_hint: "같은 키가 이 시간 안에 다시 눌리면 막습니다. 보통 30~80ms가 적당해요. 두 번 입력이 계속되면 늘리고, 빠른 연타가 씹히면 줄이세요.",
    card_sound: "알림 소리",
    play_sound: "입력을 막을 때 소리 내기",
    sound_test: "들어보기",
    sound_file_cue: "WAV 파일 (비우면 기본 알림음)",
    sound_browse: "찾아보기…",
    sound_filter: "WAV 소리 파일",
    card_app: "프로그램",
    autostart: "Windows를 켤 때 자동으로 실행",
    minimize_on_launch: "실행할 때 창 없이 트레이로 시작",
    close_to_tray: "닫기(X)를 눌러도 트레이에서 계속 실행",
    language_label: "언어 (Language)",
    save: "저장",
    status_saved: "✓ 저장했어요",
    status_dirty: "저장하지 않은 변경 사항이 있어요",
    save_failed: "설정을 파일에 저장하지 못했습니다. 디스크 여유 공간과 파일 권한을 확인한 뒤 다시 시도하세요.",

    card_detect: "감지 방식",
    measure_from_label: "간격 재는 기준",
    measure_options: &["키를 뗀 순간부터 (권장)", "키를 누른 순간부터"],
    min_chatter_label: "무시할 짧은 간격",
    min_chatter_unit: "ms 미만 (0 = 사용 안 함)",
    min_chatter_hint: "이보다 짧은 간격은 채터링이 아닌 신호로 보고 그대로 통과시킵니다. 특별한 이유가 없으면 0으로 두세요.",
    exclude_injected: "다른 프로그램이 보낸 입력은 건드리지 않기 (권장)",
    card_log: "기록",
    record_all_keys: "모든 키 입력 기록 (끄면 막은 입력만 기록)",
    log_limit_label: "최대 기록 개수",
    log_limit_unit: "개 (10~5000)",
    log_note: "키마다 다른 차단 기준은 설정 파일에 key.XX=ms 형식으로 지정할 수 있어요. 설정 파일 위치: 도구 > 설정 폴더 열기",

    menu_file: "파일(&F)",
    menu_hide: "트레이로 숨기기(&H)",
    menu_tools: "도구(&T)",
    menu_protect: "채터링 막기(&P)",
    menu_clear_log: "기록 지우기(&L)",
    menu_open_config: "설정 폴더 열기(&O)",
    menu_help: "도움말(&H)",
    menu_homepage: "홈페이지 열기(&W)",
    menu_about: "프로그램 정보(&A)",
    about_fmt: "Keyboard Chatter Fix v{}\n\n키를 한 번 눌렀는데 두 번 입력되는 '채터링'을 막아주는 프로그램입니다.\n\n설정 파일: {}\n홈페이지: {}",

    tray_tip_on: "Keyboard Chatter Fix - 보호 켜짐",
    tray_tip_off: "Keyboard Chatter Fix - 보호 꺼짐",
    tray_balloon: "창을 닫아도 여기에서 계속 보호하고 있어요. 아이콘을 클릭하면 다시 열립니다.",
    menu_show: "창 열기(&O)",
    menu_disable: "보호 잠시 끄기",
    menu_enable: "보호 다시 켜기",
    menu_cleaning: "키보드 청소 모드(&C)",
    menu_update: "업데이트 확인(&U)…",
    menu_exit: "종료(&X)",

    already_running: "프로그램이 이미 실행 중입니다. 작업 표시줄 오른쪽 트레이 아이콘을 확인하세요.",
    reset_confirm: "입력 통계와 기록을 모두 0으로 되돌릴까요?\n이 작업은 되돌릴 수 없습니다.",
    update_title: "업데이트",
    update_failed_title: "업데이트 실패",
    update_latest: "현재 최신 버전(v{})을 사용 중입니다.",
    update_available: "새 버전 v{}이(가) 있습니다. 지금 업데이트할까요?\n(현재 버전: v{})\n\n업데이트하면 프로그램이 잠시 종료된 뒤 자동으로 다시 시작됩니다.",
    hook_failed: "키보드 입력을 감시하지 못해 채터링 방지가 동작하지 않습니다. 프로그램을 다시 시작해 보세요.",
    autostart_failed: "Windows 시작 시 자동 실행 설정을 바꾸지 못했습니다. 다시 시도해 보세요.",
    update_busy: "이미 업데이트를 확인하고 있습니다.",
};

static EN: Strings = Strings {
    title: "Keyboard Chatter Fix",
    subtitle: "Stops keys from typing twice when pressed once",
    status_on: "● Protection on",
    status_off: "○ Protection off",
    tab_home: "Home",
    tab_history: "History",
    tab_settings: "Settings",
    tab_advanced: "Advanced",
    footer_tray: "v{} · Keeps protecting from the tray when closed",
    footer_exit: "v{} · Closing the window exits the program",

    enable_filter: "Block chatter",
    home_desc: "When the same key repeats within a split second, it is treated as a hardware glitch (chatter) and blocked. Normal typing is unaffected.",
    card_stats: "Statistics",
    stat_total: "Key presses",
    stat_blocked: "Blocked",
    stat_ratio: "Blocked rate",
    last_blocked_fmt: "Last blocked: {} (repeated after {} ms)",
    last_blocked_none: "Last blocked: none yet",
    top_chattered_fmt: "Most affected key: {} ({} blocked, {}% of its presses)",
    top_chattered_none: "Most affected key: none yet",
    card_tools: "Tools",
    clean_mode_btn: "Cleaning mode",
    reset_stats_btn: "Reset stats",
    open_cpl_btn: "Windows keyboard",
    home_tip: "Tip: right-click the tray icon to pause protection or start cleaning mode without opening this window.",

    clean_title: "Keyboard cleaning mode",
    clean_body: "All key presses are blocked right now, so you can wipe the keyboard safely.\nPress Enter or click the button below to finish.",
    clean_end_btn: "Finish cleaning",

    history_views: &["Blocked only", "All input", "Per-key summary"],
    history_search_cue: "Filter by key (e.g. Space)",
    history_clear_btn: "Clear",
    col_time: "Time",
    col_key: "Key",
    col_result: "Result",
    col_gap: "Gap",
    col_presses: "Presses",
    col_blocked: "Blocked",
    col_rate: "Rate",
    col_threshold: "Threshold",
    result_blocked: "Blocked",
    result_passed: "Passed",
    threshold_custom_fmt: "{} ms (custom)",
    history_count_fmt: "{} shown",
    history_note_blocked: "Every blocked press is listed here. The history is cleared when the program exits.",
    history_note_all_off: "Only blocked presses are being recorded. Turn on 'Record every key press' in the Advanced tab to see all input.",
    history_note_all_on: "Every key press is being recorded. The history is cleared when the program exits.",
    history_note_keys: "Per-key totals are saved and survive restarts.",

    card_threshold: "Sensitivity",
    threshold_label: "Block window",
    unit_ms: "ms",
    threshold_hint: "A repeat of the same key within this time is blocked. 30–80 ms suits most keyboards: raise it if double typing persists, lower it if fast repeats get swallowed.",
    card_sound: "Sound",
    play_sound: "Play a sound when a press is blocked",
    sound_test: "Preview",
    sound_file_cue: "WAV file (blank = system sound)",
    sound_browse: "Browse…",
    sound_filter: "WAV sound files",
    card_app: "App",
    autostart: "Start automatically with Windows",
    minimize_on_launch: "Start hidden in the tray",
    close_to_tray: "Keep running in the tray when closed (X)",
    language_label: "Language (언어)",
    save: "Save",
    status_saved: "✓ Saved",
    status_dirty: "You have unsaved changes",
    save_failed: "Could not save settings to disk. Check free space and file permissions, then try again.",

    card_detect: "Detection",
    measure_from_label: "Measure gap from",
    measure_options: &["Key release (recommended)", "Key press"],
    min_chatter_label: "Ignore gaps under",
    min_chatter_unit: "ms (0 = off)",
    min_chatter_hint: "Gaps shorter than this are treated as non-chatter signals and passed through. Leave at 0 unless you have a reason.",
    exclude_injected: "Leave input sent by other programs alone (recommended)",
    card_log: "History",
    record_all_keys: "Record every key press (off: blocked presses only)",
    log_limit_label: "Keep up to",
    log_limit_unit: "entries (10–5000)",
    log_note: "Per-key thresholds can be set in the settings file as key.XX=ms. Find the file via Tools > Open settings folder.",

    menu_file: "&File",
    menu_hide: "&Hide to tray",
    menu_tools: "&Tools",
    menu_protect: "Block &chatter",
    menu_clear_log: "Clear &history",
    menu_open_config: "&Open settings folder",
    menu_help: "&Help",
    menu_homepage: "&Website",
    menu_about: "&About",
    about_fmt: "Keyboard Chatter Fix v{}\n\nBlocks keyboard 'chatter': a key that types twice when you press it once.\n\nSettings file: {}\nWebsite: {}",

    tray_tip_on: "Keyboard Chatter Fix - protection on",
    tray_tip_off: "Keyboard Chatter Fix - protection off",
    tray_balloon: "Still protecting from here after the window closes. Click the icon to reopen it.",
    menu_show: "&Open",
    menu_disable: "Pause protection",
    menu_enable: "Resume protection",
    menu_cleaning: "Keyboard &cleaning mode",
    menu_update: "Check for &Updates…",
    menu_exit: "E&xit",

    already_running: "The program is already running. Look for its icon in the system tray.",
    reset_confirm: "Reset all statistics and history to zero?\nThis cannot be undone.",
    update_title: "Update",
    update_failed_title: "Update Failed",
    update_latest: "You are using the latest version (v{}).",
    update_available: "A new version v{} is available. Update now?\n(Current version: v{})\n\nThe program will close briefly and restart automatically.",
    hook_failed: "Keyboard input could not be monitored, so chatter protection is not working. Try restarting the program.",
    autostart_failed: "Could not change the start-with-Windows setting. Please try again.",
    update_busy: "An update check is already in progress.",
};

pub fn strings(lang: Language) -> &'static Strings {
    match lang {
        Language::Ko => &KO,
        Language::En => &EN,
    }
}

/// Fill positional `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    for arg in args {
        match rest.find("{}") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(arg);
                rest = &rest[i + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::super::language::Language;
    use super::*;

    fn placeholders(s: &str) -> usize {
        s.matches("{}").count()
    }

    /// Both languages must agree on every format string's placeholder count,
    /// otherwise one language silently drops or misplaces a value.
    #[test]
    fn format_strings_have_matching_placeholders() {
        let (ko, en) = (strings(Language::Ko), strings(Language::En));
        for (k, e, n) in [
            (ko.last_blocked_fmt, en.last_blocked_fmt, 2),
            (ko.top_chattered_fmt, en.top_chattered_fmt, 3),
            (ko.threshold_custom_fmt, en.threshold_custom_fmt, 1),
            (ko.history_count_fmt, en.history_count_fmt, 1),
            (ko.footer_tray, en.footer_tray, 1),
            (ko.footer_exit, en.footer_exit, 1),
            (ko.about_fmt, en.about_fmt, 3),
            (ko.update_latest, en.update_latest, 1),
            (ko.update_available, en.update_available, 2),
        ] {
            assert_eq!(placeholders(k), n, "ko: {k}");
            assert_eq!(placeholders(e), n, "en: {e}");
        }
    }

    #[test]
    fn option_lists_have_same_length_in_both_languages() {
        let (ko, en) = (strings(Language::Ko), strings(Language::En));
        assert_eq!(ko.history_views.len(), 3);
        assert_eq!(en.history_views.len(), 3);
        assert_eq!(ko.measure_options.len(), 2);
        assert_eq!(en.measure_options.len(), 2);
    }

    #[test]
    fn tray_tip_fits_notify_icon_buffer() {
        // NOTIFYICONDATAW.szTip holds 128 UTF-16 units including the NUL.
        for lang in [Language::Ko, Language::En] {
            let s = strings(lang);
            for tip in [s.tray_tip_on, s.tray_tip_off] {
                assert!(tip.encode_utf16().count() < 128, "{tip}");
            }
            assert!(s.tray_balloon.encode_utf16().count() < 256);
        }
    }

    #[test]
    fn fill_replaces_in_order_and_tolerates_mismatch() {
        assert_eq!(fill("{} ({}ms)", &["A", "12"]), "A (12ms)");
        // Extra args are ignored, missing args leave the placeholder.
        assert_eq!(fill("{}", &["x", "y"]), "x");
        assert_eq!(fill("{} {}", &["x"]), "x {}");
        assert_eq!(fill("none", &[]), "none");
    }
}
