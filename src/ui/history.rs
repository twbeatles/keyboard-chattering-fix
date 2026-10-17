//! History tab: blocked/all key events and a per-key summary.
//!
//! Rendering is pure (`render_events`, `render_keys`, `render_info`) and
//! unit-tested. The list control is a virtual (`LVS_OWNERDATA`) list view:
//! the timer tick only swaps the cached row vector and the item count, and
//! the control pulls cell text on demand through `LVN_GETDISPINFOW`, so a
//! 5000-entry history costs no per-item Win32 inserts.

use std::{sync::Mutex, time::Duration};

use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::InvalidateRect,
        UI::{
            Controls::*,
            WindowsAndMessaging::{GetDlgItem, SendMessageW, CB_GETCURSEL},
        },
    },
};

use super::{
    ids::{ID_HIST_INFO, ID_HIST_LIST, ID_HIST_SEARCH, ID_HIST_VIEW},
    keys::{key_matches, vk_name},
    layout::{current_page, sc, showing_cleaning, Page},
    paint::{cached_brush, theme_is_dark, ThemeColors},
    state::current_lang,
    text::{fill_combo, get_text, set_text, to_wide},
};
use crate::{
    app::state::{SHARED, START_CLOCK_MS},
    debounce::LogEvent,
    i18n::{fill, strings, Strings},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HistoryView {
    Blocked,
    All,
    Keys,
}

impl HistoryView {
    fn from_index(i: isize) -> Self {
        match i {
            1 => HistoryView::All,
            2 => HistoryView::Keys,
            _ => HistoryView::Blocked,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One list row: its cell texts and whether to draw it in the warning color.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub cells: Vec<String>,
    pub highlight: bool,
    /// The key the row is about (double-click opens it in the Keys tab).
    pub vk: Option<u8>,
}

/// `HH:MM:SS.mmm` local time for an event `elapsed` after startup, given
/// the local time of day at startup in ms. Wraps past midnight.
pub(crate) fn format_clock(base_ms: u64, elapsed: Duration) -> String {
    let t = (base_ms + elapsed.as_millis() as u64) % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        t / 3_600_000,
        t / 60_000 % 60,
        t / 1000 % 60,
        t % 1000
    )
}

/// Event rows, newest first, filtered by view and key query.
pub(crate) fn render_events(
    s: &Strings,
    events: &[LogEvent],
    view: HistoryView,
    query: &str,
    clock_base_ms: u64,
) -> Vec<Row> {
    events
        .iter()
        .rev()
        .filter(|e| view != HistoryView::Blocked || e.blocked)
        .filter(|e| key_matches(e.vk, query))
        .map(|e| Row {
            cells: vec![
                format_clock(clock_base_ms, e.time),
                vk_name(e.vk),
                (if e.blocked {
                    s.result_blocked
                } else {
                    s.result_passed
                })
                .to_string(),
                e.gap_ms
                    .map_or_else(|| "-".to_string(), |g| format!("{g} ms")),
            ],
            // In "blocked only" every row is blocked; color adds nothing.
            highlight: e.blocked && view == HistoryView::All,
            vk: Some(e.vk),
        })
        .collect()
}

/// Cumulative per-key counters plus the effective threshold of each key.
pub(crate) struct KeyTotals {
    pub presses: [u64; 256],
    pub blocked: [u64; 256],
    /// (ms, is a per-key override)
    pub thresholds: [(u64, bool); 256],
}

/// Per-key summary rows: keys that were ever pressed, most-blocked first.
pub(crate) fn render_keys(s: &Strings, t: &KeyTotals, query: &str) -> Vec<Row> {
    let mut vks: Vec<u8> = (0..=255u8)
        .filter(|&vk| t.presses[vk as usize] > 0 || t.blocked[vk as usize] > 0)
        .filter(|&vk| key_matches(vk, query))
        .collect();
    vks.sort_by(|&a, &b| {
        let (a, b) = (a as usize, b as usize);
        t.blocked[b]
            .cmp(&t.blocked[a])
            .then(t.presses[b].cmp(&t.presses[a]))
            .then(a.cmp(&b))
    });
    vks.into_iter()
        .map(|vk| {
            let i = vk as usize;
            let rate = if t.presses[i] == 0 {
                0.0
            } else {
                t.blocked[i] as f64 * 100.0 / t.presses[i] as f64
            };
            let (ms, custom) = t.thresholds[i];
            Row {
                cells: vec![
                    vk_name(vk),
                    t.presses[i].to_string(),
                    t.blocked[i].to_string(),
                    format!("{rate:.1}%"),
                    if custom {
                        fill(s.threshold_custom_fmt, &[&ms.to_string()])
                    } else {
                        format!("{ms} ms")
                    },
                ],
                highlight: t.blocked[i] > 0,
                vk: Some(vk),
            }
        })
        .collect()
}

/// The line under the list: row count plus a hint matching the view.
pub(crate) fn render_info(
    s: &Strings,
    view: HistoryView,
    shown: usize,
    record_all: bool,
) -> String {
    let note = match view {
        HistoryView::Blocked => s.history_note_blocked,
        HistoryView::All if record_all => s.history_note_all_on,
        HistoryView::All => s.history_note_all_off,
        HistoryView::Keys => s.history_note_keys,
    };
    format!(
        "{} · {}",
        fill(s.history_count_fmt, &[&shown.to_string()]),
        note
    )
}

fn csv_field(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

fn csv_row(out: &mut String, cells: &[String]) {
    let line: Vec<String> = cells.iter().map(|c| csv_field(c)).collect();
    out.push_str(&line.join(","));
    out.push_str("\r\n");
}

/// The event log (oldest first) and the per-key summary as CSV, with a
/// UTF-8 BOM so spreadsheet apps pick the right encoding.
pub(crate) fn render_csv(
    s: &Strings,
    events: &[LogEvent],
    totals: &KeyTotals,
    clock_base_ms: u64,
) -> String {
    let mut out = String::from("\u{FEFF}");
    let header = |cols: &[&str]| cols.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    csv_row(
        &mut out,
        &header(&[s.col_time, s.col_key, s.col_result, s.col_gap]),
    );
    for row in render_events(s, events, HistoryView::All, "", clock_base_ms)
        .into_iter()
        .rev()
    {
        csv_row(&mut out, &row.cells);
    }
    out.push_str("\r\n");
    csv_row(
        &mut out,
        &header(&[
            s.col_key,
            s.col_presses,
            s.col_blocked,
            s.col_rate,
            s.col_threshold,
        ]),
    );
    for row in render_keys(s, totals, "") {
        csv_row(&mut out, &row.cells);
    }
    out
}

/// CSV of what is recorded right now (File > Export history).
pub(crate) fn csv_snapshot() -> Option<String> {
    let shared = SHARED.get()?;
    let (events, totals) = {
        let g = shared.lock().unwrap();
        let d = &g.debouncer;
        let mut thresholds = [(0u64, false); 256];
        for (vk, slot) in thresholds.iter_mut().enumerate() {
            *slot = d.key_threshold_ms(vk as u8);
        }
        let events: Vec<LogEvent> = d.log_events.iter().copied().collect();
        (
            events,
            Box::new(KeyTotals {
                presses: d.per_key_presses,
                blocked: d.per_key_blocked,
                thresholds,
            }),
        )
    };
    Some(render_csv(
        strings(current_lang()),
        &events,
        &totals,
        START_CLOCK_MS.get().copied().unwrap_or(0),
    ))
}

/// Column headers: (title, logical width, right-aligned).
fn columns(s: &Strings, view: HistoryView) -> Vec<(&'static str, i32, bool)> {
    match view {
        HistoryView::Keys => vec![
            (s.col_key, 110, false),
            (s.col_presses, 76, true),
            (s.col_blocked, 70, true),
            (s.col_rate, 76, true),
            (s.col_threshold, 94, true),
        ],
        _ => vec![
            (s.col_time, 124, false),
            (s.col_key, 120, false),
            (s.col_result, 80, false),
            (s.col_gap, 100, true),
        ],
    }
}

// --- Window glue (UI thread) ---------------------------------------------

/// Change key of the last render: (log seq, total, blocked, record-all).
type RenderKey = (u64, u64, u64, bool);

struct State {
    view: HistoryView,
    query: String,
    rows: Vec<Row>,
    last_key: Option<RenderKey>,
}

static STATE: Mutex<State> = Mutex::new(State {
    view: HistoryView::Blocked,
    query: String::new(),
    rows: Vec::new(),
    last_key: None,
});

enum Snapshot {
    Events(Vec<LogEvent>),
    Keys(Box<KeyTotals>),
}

unsafe fn list(hwnd: HWND) -> Option<HWND> {
    GetDlgItem(Some(hwnd), ID_HIST_LIST).ok()
}

/// One-time list view setup after creation.
pub(crate) unsafe fn init(hwnd: HWND) {
    if let Some(lv) = list(hwnd) {
        setup_list(lv);
    }
}

/// Full-row selection, double buffering, and the themed header; shared by
/// every report list in the window.
pub(crate) unsafe fn setup_list(lv: HWND) {
    let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER;
    SendMessageW(
        lv,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(ex as usize)),
        Some(LPARAM(ex as isize)),
    );
    let header =
        HWND(SendMessageW(lv, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    dark_header::install(header);
    theme_list(lv);
}

pub(crate) unsafe fn apply_theme(hwnd: HWND) {
    if let Some(lv) = list(hwnd) {
        theme_list(lv);
    }
}

/// Colors and visual style of a report list for the current theme.
pub(crate) unsafe fn theme_list(lv: HWND) {
    let dark = theme_is_dark();
    let c = ThemeColors::new(dark);
    for msg in [LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR] {
        SendMessageW(lv, msg, Some(WPARAM(0)), Some(LPARAM(c.card_bg.0 as isize)));
    }
    SendMessageW(
        lv,
        LVM_SETTEXTCOLOR,
        Some(WPARAM(0)),
        Some(LPARAM(c.text_primary.0 as isize)),
    );
    let header =
        HWND(SendMessageW(lv, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    if dark {
        let _ = SetWindowTheme(lv, w!("DarkMode_Explorer"), PCWSTR::null());
        let _ = SetWindowTheme(header, w!("DarkMode_ItemsView"), PCWSTR::null());
    } else {
        let _ = SetWindowTheme(lv, w!("Explorer"), PCWSTR::null());
        let _ = SetWindowTheme(header, PCWSTR::null(), PCWSTR::null());
    }
    let _ = InvalidateRect(Some(lv), None, true);
    let _ = InvalidateRect(Some(header), None, true);
}

unsafe fn rebuild_columns(hwnd: HWND, view: HistoryView) {
    if let Some(lv) = list(hwnd) {
        set_columns(lv, &columns(strings(current_lang()), view));
    }
}

/// Replace every column: (title, logical width, right-aligned).
pub(crate) unsafe fn set_columns(lv: HWND, cols: &[(&str, i32, bool)]) {
    while SendMessageW(lv, LVM_DELETECOLUMN, Some(WPARAM(0)), Some(LPARAM(0))).0 != 0 {}
    for (i, &(title, width, right)) in cols.iter().enumerate() {
        let mut text = to_wide(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
            fmt: if right { LVCFMT_RIGHT } else { LVCFMT_LEFT },
            cx: sc(width),
            pszText: PWSTR(text.as_mut_ptr()),
            ..Default::default()
        };
        SendMessageW(
            lv,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(i)),
            Some(LPARAM(&col as *const _ as isize)),
        );
    }
}

/// Retranslate the view selector, search hint, and columns.
pub(crate) unsafe fn apply_language(hwnd: HWND) {
    let s = strings(current_lang());
    let view = STATE.lock().unwrap().view;
    fill_combo(hwnd, ID_HIST_VIEW, s.history_views, view.index());
    if let Ok(edit) = GetDlgItem(Some(hwnd), ID_HIST_SEARCH) {
        let cue = to_wide(s.history_search_cue);
        SendMessageW(
            edit,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
    rebuild_columns(hwnd, view);
    refresh(hwnd, true);
}

pub(crate) unsafe fn on_view_changed(hwnd: HWND) {
    let sel = GetDlgItem(Some(hwnd), ID_HIST_VIEW)
        .map(|h| SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0)
        .unwrap_or(0);
    let view = HistoryView::from_index(sel);
    STATE.lock().unwrap().view = view;
    rebuild_columns(hwnd, view);
    refresh(hwnd, true);
}

pub(crate) unsafe fn on_search_changed(hwnd: HWND) {
    STATE.lock().unwrap().query = get_text(hwnd, ID_HIST_SEARCH);
    refresh(hwnd, true);
}

/// Force the next refresh to re-render (thresholds or language changed).
pub(crate) fn invalidate() {
    STATE.lock().unwrap().last_key = None;
}

/// Re-render when the History tab is visible and something changed.
/// Locks are taken one at a time and released before any Win32 call.
pub(crate) unsafe fn refresh(hwnd: HWND, force: bool) {
    if current_page() != Page::History || showing_cleaning() {
        return;
    }
    let (view, query, last_key) = {
        let st = STATE.lock().unwrap();
        (st.view, st.query.clone(), st.last_key)
    };
    let Some(shared) = SHARED.get() else {
        return;
    };
    let (key, snapshot) = {
        let g = shared.lock().unwrap();
        let d = &g.debouncer;
        let key: RenderKey = match view {
            HistoryView::Keys => (
                d.log_seq,
                d.total_presses,
                d.blocked_presses,
                d.record_all_keys,
            ),
            _ => (d.log_seq, 0, 0, d.record_all_keys),
        };
        if !force && last_key == Some(key) {
            return;
        }
        let snapshot = match view {
            HistoryView::Keys => {
                let mut thresholds = [(0u64, false); 256];
                for (vk, slot) in thresholds.iter_mut().enumerate() {
                    *slot = d.key_threshold_ms(vk as u8);
                }
                Snapshot::Keys(Box::new(KeyTotals {
                    presses: d.per_key_presses,
                    blocked: d.per_key_blocked,
                    thresholds,
                }))
            }
            _ => Snapshot::Events(d.log_events.iter().copied().collect()),
        };
        (key, snapshot)
    };

    let s = strings(current_lang());
    let rows = match &snapshot {
        Snapshot::Events(events) => render_events(
            s,
            events,
            view,
            &query,
            START_CLOCK_MS.get().copied().unwrap_or(0),
        ),
        Snapshot::Keys(totals) => render_keys(s, totals, &query),
    };
    let count = rows.len();
    {
        let mut st = STATE.lock().unwrap();
        st.rows = rows;
        st.last_key = Some(key);
    }

    if let Some(lv) = list(hwnd) {
        SendMessageW(
            lv,
            LVM_SETITEMCOUNT,
            Some(WPARAM(count)),
            Some(LPARAM(LVSICF_NOSCROLL as isize)),
        );
        let _ = InvalidateRect(Some(lv), None, false);
    }
    set_text(hwnd, ID_HIST_INFO, &render_info(s, view, count, key.3));
}

/// Dark-mode column headers. The themed header ignores `SetTextColor`
/// (black text on the dark list) and the list view swallows custom-draw
/// results for it, so the header is subclassed and painted here whenever
/// the app theme is dark. Light mode defers to the system look.
mod dark_header {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreatePen, DeleteObject, DrawTextW, EndPaint, FillRect, LineTo,
                MoveToEx, SelectObject, SetBkMode, SetTextColor, DT_END_ELLIPSIS, DT_LEFT,
                DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, HFONT, PAINTSTRUCT, PS_SOLID,
                TRANSPARENT,
            },
            UI::{
                Controls::*,
                Shell::{DefSubclassProc, SetWindowSubclass},
                WindowsAndMessaging::{GetClientRect, WM_ERASEBKGND, WM_GETFONT, WM_PAINT},
            },
        },
    };

    use super::{cached_brush, sc, theme_is_dark, ThemeColors};

    const SUBCLASS_ID: usize = 0x4B43_4844; // "KCHD"

    pub(super) unsafe fn install(header: HWND) {
        if !header.is_invalid() {
            let _ = SetWindowSubclass(header, Some(proc), SUBCLASS_ID, 0);
        }
    }

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if theme_is_dark() {
            match msg {
                WM_ERASEBKGND => return LRESULT(1),
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    paint(hwnd, hdc);
                    let _ = EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    unsafe fn paint(header: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
        let c = ThemeColors::new(true);
        let mut client = RECT::default();
        let _ = GetClientRect(header, &mut client);
        // Background, including the strip right of the last column.
        FillRect(hdc, &client, cached_brush(c.tab_track));

        let font = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            header,
            WM_GETFONT,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        );
        let old_font = SelectObject(hdc, HFONT(font.0 as *mut _).into());
        let _ = SetTextColor(hdc, c.text_secondary);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let pen = CreatePen(PS_SOLID, 1, c.card_border);
        let old_pen = SelectObject(hdc, pen.into());

        use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
        let count = SendMessageW(header, HDM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
        for index in 0..count.max(0) as usize {
            let mut item_rc = RECT::default();
            SendMessageW(
                header,
                HDM_GETITEMRECT,
                Some(WPARAM(index)),
                Some(LPARAM(&mut item_rc as *mut _ as isize)),
            );
            let mut text = [0u16; 64];
            let mut item = HDITEMW {
                mask: HDI_TEXT | HDI_FORMAT,
                pszText: PWSTR(text.as_mut_ptr()),
                cchTextMax: text.len() as i32,
                ..Default::default()
            };
            SendMessageW(
                header,
                HDM_GETITEMW,
                Some(WPARAM(index)),
                Some(LPARAM(&mut item as *mut _ as isize)),
            );
            let len = text.iter().position(|&u| u == 0).unwrap_or(text.len());
            let mut rc = item_rc;
            rc.left += sc(8);
            rc.right -= sc(8);
            let align = if item.fmt.0 & HDF_RIGHT.0 != 0 {
                DT_RIGHT
            } else {
                DT_LEFT
            };
            DrawTextW(
                hdc,
                &mut text[..len],
                &mut rc,
                align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            // Thin divider on the item's right edge.
            let _ = MoveToEx(hdc, item_rc.right - 1, item_rc.top + sc(4), None);
            let _ = LineTo(hdc, item_rc.right - 1, item_rc.bottom - sc(4));
        }
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
        SelectObject(hdc, old_font);
    }
}

/// Copy `text` into the list's cell buffer (truncated, NUL-terminated).
/// The caller has checked that the buffer exists.
pub(crate) unsafe fn write_cell(info: &mut NMLVDISPINFOW, text: &str) {
    let buf = std::slice::from_raw_parts_mut(info.item.pszText.0, info.item.cchTextMax as usize);
    let mut n = 0;
    for unit in text.encode_utf16() {
        if n + 1 >= buf.len() {
            break;
        }
        buf[n] = unit;
        n += 1;
    }
    buf[n] = 0;
}

/// The key of the row double-clicked in the per-key summary, if that is
/// what this `WM_NOTIFY` is.
pub(crate) unsafe fn double_clicked_vk(lparam: LPARAM) -> Option<u8> {
    let hdr = &*(lparam.0 as *const windows::Win32::UI::Controls::NMHDR);
    if hdr.idFrom != ID_HIST_LIST as usize || hdr.code != NM_DBLCLK {
        return None;
    }
    let item = (*(lparam.0 as *const NMITEMACTIVATE)).iItem;
    let st = STATE.lock().unwrap();
    if st.view != HistoryView::Keys || item < 0 {
        return None;
    }
    st.rows.get(item as usize).and_then(|r| r.vk)
}

/// `WM_NOTIFY` from the list: serve cell text and color blocked rows.
pub(crate) unsafe fn on_notify(lparam: LPARAM) -> Option<LRESULT> {
    let hdr = &*(lparam.0 as *const windows::Win32::UI::Controls::NMHDR);
    if hdr.idFrom != ID_HIST_LIST as usize {
        return None;
    }
    match hdr.code {
        LVN_GETDISPINFOW => {
            let info = &mut *(lparam.0 as *mut NMLVDISPINFOW);
            if (info.item.mask & LVIF_TEXT).0 != 0
                && !info.item.pszText.is_null()
                && info.item.cchTextMax > 0
            {
                let st = STATE.lock().unwrap();
                let text = st
                    .rows
                    .get(info.item.iItem as usize)
                    .and_then(|r| r.cells.get(info.item.iSubItem as usize))
                    .map(String::as_str)
                    .unwrap_or("");
                write_cell(info, text);
            }
            Some(LRESULT(0))
        }
        NM_CUSTOMDRAW => {
            let cd = &mut *(lparam.0 as *mut NMLVCUSTOMDRAW);
            if cd.nmcd.dwDrawStage == CDDS_PREPAINT {
                return Some(LRESULT(CDRF_NOTIFYITEMDRAW as isize));
            }
            if cd.nmcd.dwDrawStage == CDDS_ITEMPREPAINT {
                let highlight = STATE
                    .lock()
                    .unwrap()
                    .rows
                    .get(cd.nmcd.dwItemSpec)
                    .is_some_and(|r| r.highlight);
                if highlight {
                    cd.clrText = ThemeColors::new(theme_is_dark()).warn;
                    return Some(LRESULT(CDRF_NEWFONT as isize));
                }
            }
            Some(LRESULT(CDRF_DODEFAULT as isize))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn ev(ms: u64, vk: u8, blocked: bool, gap: Option<u64>) -> LogEvent {
        LogEvent {
            time: Duration::from_millis(ms),
            vk,
            blocked,
            gap_ms: gap,
        }
    }

    fn sample() -> Vec<LogEvent> {
        vec![
            ev(1_000, 0x41, false, None),
            ev(1_020, 0x41, true, Some(12)),
            ev(2_000, 0x20, true, Some(30)),
            ev(3_000, 0x42, false, None),
        ]
    }

    #[test]
    fn clock_formats_and_wraps_past_midnight() {
        assert_eq!(format_clock(0, Duration::ZERO), "00:00:00.000");
        let base = (13 * 3600 + 5 * 60 + 9) * 1000 + 7;
        assert_eq!(
            format_clock(base, Duration::from_millis(1_000)),
            "13:05:10.007"
        );
        let almost_midnight = 86_400_000 - 500;
        assert_eq!(
            format_clock(almost_midnight, Duration::from_millis(1_500)),
            "00:00:01.000"
        );
    }

    #[test]
    fn blocked_view_lists_only_blocked_newest_first() {
        let s = strings(Language::Ko);
        let rows = render_events(s, &sample(), HistoryView::Blocked, "", 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells, ["00:00:02.000", "Space", "막음", "30 ms"]);
        assert_eq!(rows[1].cells[1], "A");
        assert!(rows.iter().all(|r| !r.highlight));
    }

    #[test]
    fn all_view_highlights_blocked_rows_and_shows_dash_gap() {
        let s = strings(Language::En);
        let rows = render_events(s, &sample(), HistoryView::All, "", 0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].cells, ["00:00:03.000", "B", "Passed", "-"]);
        let highlighted: Vec<bool> = rows.iter().map(|r| r.highlight).collect();
        assert_eq!(highlighted, [false, true, true, false]);
    }

    #[test]
    fn key_query_filters_events() {
        let s = strings(Language::En);
        let rows = render_events(s, &sample(), HistoryView::All, "space", 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells[1], "Space");
        assert!(render_events(s, &sample(), HistoryView::All, "F12", 0).is_empty());
    }

    fn totals() -> KeyTotals {
        let mut t = KeyTotals {
            presses: [0; 256],
            blocked: [0; 256],
            thresholds: [(50, false); 256],
        };
        t.presses[0x41] = 100;
        t.blocked[0x41] = 5;
        t.presses[0x20] = 40;
        t.blocked[0x20] = 10;
        t.presses[0x42] = 300;
        t.thresholds[0x20] = (80, true);
        t
    }

    #[test]
    fn key_summary_sorts_by_blocked_and_marks_overrides() {
        let s = strings(Language::Ko);
        let rows = render_keys(s, &totals(), "");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0].cells,
            ["Space", "40", "10", "25.0%", "80 ms (개별)"]
        );
        assert_eq!(rows[1].cells, ["A", "100", "5", "5.0%", "50 ms"]);
        assert_eq!(rows[2].cells[0], "B");
        assert_eq!(
            rows.iter().map(|r| r.highlight).collect::<Vec<_>>(),
            [true, true, false]
        );
    }

    #[test]
    fn key_summary_skips_unused_keys_and_honours_query() {
        let s = strings(Language::En);
        let empty = KeyTotals {
            presses: [0; 256],
            blocked: [0; 256],
            thresholds: [(50, false); 256],
        };
        assert!(render_keys(s, &empty, "").is_empty());
        let rows = render_keys(s, &totals(), "b");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells[0], "B");
    }

    #[test]
    fn info_line_explains_missing_all_input_recording() {
        let s = strings(Language::Ko);
        let off = render_info(s, HistoryView::All, 3, false);
        assert!(off.starts_with("3개 표시 · "));
        assert!(off.contains("모든 키 입력 기록"));
        let on = render_info(s, HistoryView::All, 3, true);
        assert_ne!(on, off);
        assert!(render_info(s, HistoryView::Keys, 0, false).contains("유지"));
    }

    #[test]
    fn csv_lists_events_oldest_first_then_the_key_summary() {
        let s = strings(Language::En);
        let csv = render_csv(s, &sample(), &totals(), 0);
        assert!(csv.starts_with('\u{FEFF}'));
        let lines: Vec<&str> = csv.trim_start_matches('\u{FEFF}').split("\r\n").collect();
        assert_eq!(lines[0], "Time,Key,Result,Gap");
        assert_eq!(lines[1], "00:00:01.000,A,Passed,-");
        assert_eq!(lines[4], "00:00:03.000,B,Passed,-");
        assert_eq!(lines[5], "");
        assert_eq!(lines[6], "Key,Presses,Blocked,Rate,Threshold");
        assert_eq!(lines[7], "Space,40,10,25.0%,80 ms (custom)");
        // Fields with separators or quotes are quoted.
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field(","), "\",\"");
    }

    #[test]
    fn column_count_matches_rendered_cells() {
        let s = strings(Language::En);
        let events = render_events(s, &sample(), HistoryView::All, "", 0);
        assert_eq!(columns(s, HistoryView::All).len(), events[0].cells.len());
        let keys = render_keys(s, &totals(), "");
        assert_eq!(columns(s, HistoryView::Keys).len(), keys[0].cells.len());
    }
}
