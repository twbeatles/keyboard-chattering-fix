//! Localizable UI strings (Korean / English).
//!
//! Wording rule: describe what the user sees ("a key typed twice"), not the
//! mechanism ("debounce threshold"). Placeholders are positional `{}`.

use super::language::Language;

/// Declares `Strings` and, for tests, a listing of every field, so
/// "no empty/missing translation" is checked for all fields without a
/// hand-kept list that could drift.
macro_rules! define_strings {
    (
        texts: [$($text:ident),* $(,)?],
        lists: [$($list:ident),* $(,)?] $(,)?
    ) => {
        pub struct Strings {
            $(pub $text: &'static str,)*
            $(pub $list: &'static [&'static str],)*
        }

        #[cfg(test)]
        impl Strings {
            /// (field name, text) for every single-line field.
            pub fn texts(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($text), self.$text)),*]
            }

            /// (field name, items) for every list field.
            pub fn lists(&self) -> Vec<(&'static str, &'static [&'static str])> {
                vec![$((stringify!($list), self.$list)),*]
            }
        }
    };
}

define_strings! {
    texts: [
        title,
        subtitle,
        status_on,
        status_off,
        tab_home,
        tab_history,
        tab_settings,
        tab_advanced,
        tab_keys,
        footer_tray,
        footer_exit,
        enable_filter,
        home_desc,
        card_stats,
        stat_total,
        stat_blocked,
        stat_ratio,
        last_blocked_fmt,
        last_blocked_none,
        top_chattered_fmt,
        top_chattered_none,
        card_tools,
        clean_mode_btn,
        reset_stats_btn,
        open_cpl_btn,
        home_tip,
        clean_title,
        clean_body,
        clean_end_btn,
        history_search_cue,
        history_clear_btn,
        col_time,
        col_key,
        col_result,
        col_gap,
        col_presses,
        col_blocked,
        col_rate,
        col_threshold,
        result_blocked,
        result_passed,
        threshold_custom_fmt,
        history_count_fmt,
        history_note_blocked,
        history_note_all_off,
        history_note_all_on,
        history_note_keys,
        card_threshold,
        threshold_label,
        unit_ms,
        threshold_hint,
        card_sound,
        play_sound,
        sound_test,
        sound_file_cue,
        sound_browse,
        sound_filter,
        card_app,
        autostart,
        minimize_on_launch,
        close_to_tray,
        language_label,
        language_auto,
        save,
        status_saved,
        status_dirty,
        save_failed,
        card_detect,
        measure_from_label,
        min_chatter_label,
        min_chatter_unit,
        min_chatter_hint,
        exclude_injected,
        card_log,
        record_all_keys,
        log_limit_label,
        log_limit_unit,
        log_note,
        filter_mouse,
        mouse_hint,
        card_keys,
        col_reco,
        key_set_btn,
        key_reset_btn,
        key_stats_btn,
        keys_note,
        advice_pick,
        advice_need_fmt,
        advice_ok_fmt,
        advice_suggest_fmt,
        advice_suggest_only_fmt,
        advice_overlap_fmt,
        reco_cell_need,
        reco_cell_ok,
        reco_cell_overlap,
        menu_file,
        menu_hide,
        menu_tools,
        menu_protect,
        menu_clear_log,
        menu_open_config,
        menu_help,
        menu_homepage,
        menu_about,
        about_fmt,
        tray_tip_on,
        tray_tip_off,
        tray_balloon,
        menu_show,
        menu_disable,
        menu_enable,
        menu_cleaning,
        menu_update,
        menu_exit,
        already_running,
        reset_confirm,
        update_title,
        update_failed_title,
        update_latest,
        update_available,
        hook_failed,
        autostart_failed,
        update_busy,
        auto_apps_label,
        auto_apps_cue,
        auto_fullscreen,
        defer_label,
        defer_unit,
        defer_hint,
        hotkeys_label,
        hotkey_toggle_cue,
        hotkey_clean_cue,
        hotkey_invalid_fmt,
        hotkey_taken_fmt,
        status_paused,
        note_suspended_fmt,
        note_fullscreen_fmt,
        note_elevated,
        note_hook_restarted_fmt,
        tray_tip_paused,
        menu_export_history,
        menu_export_settings,
        menu_import_settings,
        menu_run_elevated,
        menu_elevated_autostart,
        csv_filter,
        ini_filter,
        export_done_fmt,
        export_failed,
        import_invalid,
        import_done,
        elevated_autostart_failed,
        cleaning_auto_ended,
        update_managed_fmt,
    ],
    lists: [
        history_views,
        measure_options,
    ],
}

static KO: Strings = Strings {
    title: "키보드 채터링 방지",
    subtitle: "한 번 누른 키가 두 번 입력되는 현상을 막아줍니다",
    status_on: "● 보호 켜짐",
    status_off: "○ 보호 꺼짐",
    tab_home: "홈",
    tab_history: "기록",
    tab_settings: "설정",
    tab_advanced: "고급",
    tab_keys: "키·마우스",
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
    clean_body: "지금은 모든 키 입력을 막고 있어요. 안심하고 키보드를 닦으세요.\n끝내려면 Enter 키나 아래 버튼을 누르세요. 잊어도 몇 분 뒤 자동으로 끝납니다.",
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
    history_note_keys: "키별 누적 통계는 저장되어 다시 실행해도 유지됩니다. 행을 두 번 클릭하면 그 키의 기준을 바꿀 수 있어요.",

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
    language_auto: "자동 (Windows 언어)",
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
    log_note: "키마다 다른 차단 기준과 마우스 버튼 막기는 [키·마우스] 탭에서 정할 수 있어요.",

    filter_mouse: "마우스 버튼도 막기 (클릭이 두 번 되는 문제)",
    mouse_hint: "모든 마우스 버튼에 같은 기준을 쓰고 클릭도 통계에 셉니다. 더블클릭이 안 되면 아래 목록에서 버튼별 기준을 낮추세요.",
    card_keys: "키별 차단 기준",
    col_reco: "권장",
    key_set_btn: "적용",
    key_reset_btn: "기본값으로",
    key_stats_btn: "통계 지우기",
    keys_note: "적용하면 바로 저장됩니다. 권장값은 이번 실행에서 잰 입력 간격으로 계산해요.",
    advice_pick: "목록에서 키를 고르면 그 키에 맞는 권장 기준을 보여드려요.",
    advice_need_fmt: "{}: 권장값을 계산하려면 이 키를 더 눌러야 해요 ({}/{}회).",
    advice_ok_fmt: "{}: 채터링으로 볼 만큼 빠른 재입력이 없었어요. 지금 기준이면 충분해요.",
    advice_suggest_fmt: "{}: 권장 {} ms — 튄 입력은 모두 {} ms 미만, 평소 입력은 {} ms 이상이었어요.",
    advice_suggest_only_fmt: "{}: 권장 {} ms — 튄 입력은 모두 {} ms 미만이었어요.",
    advice_overlap_fmt: "{}: 튄 입력과 평소 입력의 간격이 겹쳐 기준만으로는 구분이 어려워요. 스위치 청소나 교체를 권장해요.",
    reco_cell_need: "데이터 부족",
    reco_cell_ok: "문제 없음",
    reco_cell_overlap: "스위치 점검",

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
    auto_apps_label: "멈출 프로그램",
    auto_apps_cue: "예: game.exe; obs64.exe",
    auto_fullscreen: "전체 화면 앱이 앞에 있으면 잠시 멈추기 (게임 등)",
    defer_label: "떼기 확인 시간",
    defer_unit: "ms (0 = 끔, 보통 20~40)",
    defer_hint: "누르고 있는 키·버튼이 잠깐 튀어도 떼어진 것으로 보이지 않게, 뗌을 이 시간만큼 지켜본 뒤 보냅니다. 게임 이동 키나 드래그가 끊길 때 켜세요.",
    hotkeys_label: "단축키",
    hotkey_toggle_cue: "보호 켜기/끄기",
    hotkey_clean_cue: "청소 모드",
    hotkey_invalid_fmt: "단축키 '{}'를 알아보지 못했어요. Ctrl, Alt, Shift, Win 중 하나 이상과 키를 +로 이어 적어 주세요 (예: Ctrl+Alt+F12).",
    hotkey_taken_fmt: "단축키 {}를 등록하지 못했어요. 다른 프로그램이 이미 쓰고 있을 수 있어요.",
    status_paused: "◐ 잠시 멈춤",
    note_suspended_fmt: "{}이(가) 앞에 있어 채터링 막기를 잠시 멈췄어요. 다른 창으로 돌아가면 다시 켜집니다.",
    note_fullscreen_fmt: "전체 화면 앱({})이 앞에 있어 채터링 막기를 잠시 멈췄어요.",
    note_elevated: "관리자 권한으로 실행된 창이 앞에 있어 그 창에서는 채터링이 걸러지지 않아요. 도구 > 관리자 권한으로 다시 실행으로 해결할 수 있어요.",
    note_hook_restarted_fmt: "{}에 키보드 감시가 멈춘 것을 발견해 다시 시작했어요.",
    tray_tip_paused: "Keyboard Chatter Fix - 잠시 멈춤",
    menu_export_history: "기록 내보내기(CSV)(&E)…",
    menu_export_settings: "설정 내보내기(&S)…",
    menu_import_settings: "설정 가져오기(&I)…",
    menu_run_elevated: "관리자 권한으로 다시 실행(&R)",
    menu_elevated_autostart: "로그인할 때 관리자 권한으로 자동 실행",
    csv_filter: "CSV 파일",
    ini_filter: "설정 파일",
    export_done_fmt: "저장했어요:\n{}",
    export_failed: "파일을 저장하지 못했어요. 위치와 권한을 확인해 주세요.",
    import_invalid: "이 프로그램의 설정 파일이 아니에요.",
    import_done: "설정을 가져와 적용했어요. 입력 통계는 그대로 두었습니다.",
    elevated_autostart_failed: "관리자 권한 자동 실행 설정을 바꾸지 못했어요.",
    cleaning_auto_ended: "청소 모드를 자동으로 끝냈어요.",
    update_managed_fmt: "새 버전 v{}이(가) 있습니다.\n이 프로그램은 {}(으)로 설치되어 있으니 다음 명령으로 업데이트하세요:\n\n{}",
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
    tab_keys: "Keys/Mouse",
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
    clean_body: "All key presses are blocked so you can wipe the keyboard safely.\nPress Enter or the button below to finish; it also ends by itself after a few minutes.",
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
    history_note_keys: "Per-key totals are saved and survive restarts. Double-click a row to change that key's window.",

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
    language_auto: "Auto (Windows language)",
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
    log_note: "Per-key block windows and the mouse button filter are in the Keys/Mouse tab.",

    filter_mouse: "Also filter mouse buttons (clicks registering twice)",
    mouse_hint: "All buttons share the block window and clicks count in the statistics. If double-clicks stop working, lower it per button below.",
    card_keys: "Per-key block window",
    col_reco: "Suggested",
    key_set_btn: "Apply",
    key_reset_btn: "Default",
    key_stats_btn: "Clear stats",
    keys_note: "Changes are saved right away. Suggestions come from the gaps measured since the program started.",
    advice_pick: "Pick a key in the list to see a suggested window for it.",
    advice_need_fmt: "{}: press this key a few more times for a suggestion ({}/{}).",
    advice_ok_fmt: "{}: no repeat fast enough to be chatter so far. The current window is fine.",
    advice_suggest_fmt: "{}: suggested {} ms. Every bounce was under {} ms; real presses started at {} ms.",
    advice_suggest_only_fmt: "{}: suggested {} ms. Every bounce was under {} ms.",
    advice_overlap_fmt: "{}: bounces and real presses overlap, so no window can separate them. Clean or replace the switch.",
    reco_cell_need: "Need data",
    reco_cell_ok: "OK",
    reco_cell_overlap: "Check switch",

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
    auto_apps_label: "Pause for apps",
    auto_apps_cue: "e.g. game.exe; obs64.exe",
    auto_fullscreen: "Pause while a full-screen app is in front (games etc.)",
    defer_label: "Release delay",
    defer_unit: "ms (0 = off, typically 20–40)",
    defer_hint: "Holds each release back this long, so a held key or button that bounces never looks released. Use it when keys or drags drop in games.",
    hotkeys_label: "Hotkeys",
    hotkey_toggle_cue: "Toggle protection",
    hotkey_clean_cue: "Cleaning mode",
    hotkey_invalid_fmt: "The hotkey '{}' was not understood. Join one or more of Ctrl, Alt, Shift, Win and a key with + (e.g. Ctrl+Alt+F12).",
    hotkey_taken_fmt: "The hotkey {} could not be registered. Another program may already use it.",
    status_paused: "◐ Paused",
    note_suspended_fmt: "Protection is paused while {} is in front. It resumes when you switch away.",
    note_fullscreen_fmt: "Protection is paused while a full-screen app ({}) is in front.",
    note_elevated: "A window running as administrator is in front, so its input is not filtered. Use Tools > Run as administrator to cover it.",
    note_hook_restarted_fmt: "Keyboard monitoring stopped at {} and was restarted.",
    tray_tip_paused: "Keyboard Chatter Fix - paused",
    menu_export_history: "&Export history (CSV)…",
    menu_export_settings: "Export &settings…",
    menu_import_settings: "&Import settings…",
    menu_run_elevated: "&Run as administrator",
    menu_elevated_autostart: "Start as administrator at sign-in",
    csv_filter: "CSV files",
    ini_filter: "Settings files",
    export_done_fmt: "Saved:\n{}",
    export_failed: "Could not save the file. Check the location and permissions.",
    import_invalid: "This is not a settings file of this program.",
    import_done: "Settings imported and applied. Statistics were kept.",
    elevated_autostart_failed: "Could not change the start-as-administrator setting.",
    cleaning_auto_ended: "Cleaning mode ended automatically.",
    update_managed_fmt: "A new version v{} is available.\nThis copy was installed with {}, so update it with:\n\n{}",
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

    fn others() -> impl Iterator<Item = Language> {
        Language::ALL
            .iter()
            .copied()
            .filter(|l| *l != Language::FALLBACK)
    }

    /// Every field of every language is filled in. A forgotten placeholder
    /// such as an empty string would otherwise show up as a blank label.
    #[test]
    fn every_text_of_every_language_is_filled_in() {
        for lang in Language::ALL {
            let s = strings(*lang);
            for (name, text) in s.texts() {
                assert!(!text.trim().is_empty(), "{lang:?}.{name} is empty");
            }
            for (name, items) in s.lists() {
                assert!(!items.is_empty(), "{lang:?}.{name} has no items");
                assert!(
                    items.iter().all(|i| !i.trim().is_empty()),
                    "{lang:?}.{name}"
                );
            }
        }
    }

    /// A translation must carry the same `{}` placeholders as the English
    /// reference, otherwise a value is silently dropped or misplaced.
    #[test]
    fn every_language_keeps_the_reference_placeholders() {
        let reference = strings(Language::FALLBACK);
        for lang in others() {
            let s = strings(lang);
            for ((name, want), (_, got)) in reference.texts().into_iter().zip(s.texts()) {
                assert_eq!(
                    placeholders(got),
                    placeholders(want),
                    "{lang:?}.{name}: {got}"
                );
            }
        }
    }

    /// Every field is translated, not copied from English. Short brand-like
    /// or numeric texts may legitimately match (`Home`/`홈` aside), so only
    /// sentence-length text is required to differ.
    #[test]
    fn long_texts_are_actually_translated() {
        let reference = strings(Language::FALLBACK);
        for lang in others() {
            let s = strings(lang);
            for ((name, want), (_, got)) in reference.texts().into_iter().zip(s.texts()) {
                if want.chars().count() > 24 {
                    assert_ne!(got, want, "{lang:?}.{name} is still the English text");
                }
            }
        }
    }

    #[test]
    fn option_lists_have_same_length_in_every_language() {
        let reference = strings(Language::FALLBACK);
        for lang in others() {
            let s = strings(lang);
            for ((name, want), (_, got)) in reference.lists().into_iter().zip(s.lists()) {
                assert_eq!(got.len(), want.len(), "{lang:?}.{name}");
            }
        }
        assert_eq!(reference.history_views.len(), 3);
        assert_eq!(reference.measure_options.len(), 2);
    }

    #[test]
    fn tray_tip_fits_notify_icon_buffer() {
        // NOTIFYICONDATAW.szTip holds 128 UTF-16 units including the NUL.
        for lang in Language::ALL.iter().copied() {
            let s = strings(lang);
            for tip in [s.tray_tip_on, s.tray_tip_off, s.tray_tip_paused] {
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
