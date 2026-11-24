# Project Audit

- **대상:** `keyboard-chattering-fix` (`kcfix-rs`). 메인 바이너리와 `crates/kcfix-updater` 헬퍼로 이루어진 Cargo 워크스페이스.
- **기준 시점:** 2026-11-24. HEAD는 v0.3.6 릴리스 뒤 매니페스트 갱신 커밋이며, 작업 트리는 커밋되지 않은 변경분을 포함한다(휠 튐 필터, 실행 중 인스턴스 CLI 제어, 자동 업데이트 확인 — 모두 `Unreleased`로 `CHANGELOG.md`에 기록됨). 감사는 이 작업 트리 상태를 기준으로 했다.
- **방법:**
  1. 문서 확인 (`README.md`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `CHANGELOG.md`, `docs/UPDATE.md`, `Cargo.toml`, `build.rs`, `scripts/e2e/README.md`)
  2. `muse.search` + 파일 직접 열람으로 entrypoint·핵심 모듈·caller/callee 추적 (CodeGraph는 사용하지 못했으므로 3장에 명기)
  3. 핵심 흐름 end-to-end 추적 (입력 → 판정 → 상태 → 파일 → 결과)
  4. 후보 문제마다 반증 시도 (상위 caller 검증, 보호 장치, 도달 가능성, 테스트 여부)
  5. `cargo test --workspace` 실행 (252건)
- **코드는 수정하지 않았다.** 이 리포트(`PROJECT_AUDIT.md`)만 새로 작성한다.

---

## 1. Executive Summary

### 전체 상태

핵심 경로는 정확하고 방어적으로 짜여 있다.

- **필터 코어:** `Debouncer::decide_event`는 이벤트당 O(1). 휠 판정(`decide_wheel`)도 방향·시각 두 값만 보고 분기한다.
- **훅:** 전용 스레드 + 세대 인계 + 인젝터 스레드 분리. 마우스 훅은 버튼/휠이 원할 때만 설치된다.
- **설정 저장:** tmp → `sync_all` → 유효한 본 파일만 `.bak` 복사 → `MOVEFILE_WRITE_THROUGH` rename. 손상 판정·백업 폴백이 있다.
- **업데이트:** 서명 → 핀 고정 URL → 크기·이중 해시 → 헬퍼 교체·롤백. 검사 타임아웃(30/60/90s)과 단일 비행(single-flight) 가드가 있다.
- **이전 감사(2026-10-22, v0.3.3 기준) 지적 5건은 모두 해소된 채로 유지됨**을 코드로 재확인했다 (2장 하단 표).

### 전체 위험도: Low

Critical·High에 해당하는 문제를 찾지 못했다. 새로 발견한 문제는 **Confirmed 3건, 모두 Low**다 (표시 불일치 1, 트레이 풍선 라우팅 1, 저장 실패 시 검사 폭주 1). 데이터 유실 경로는 찾지 못했다.

### 가장 중요한 문제

1. **[ISSUE-001] [키·마우스] 탭이 Mouse Wheel의 차단 기준을 전역값(35ms)으로 보여주지만 실제 필터는 80ms로 동작한다** (Low / Confirmed)
2. **[ISSUE-002] 업데이트 풍선이 사라진 뒤에도 클릭 플래그가 남아, 이후 다른 풍선 클릭이 업데이트 흐름을 연다** (Low / Confirmed)
3. **[ISSUE-003] 설정 파일 저장이 계속 실패하면 자동 업데이트 검사가 매 틱마다(사실상 연속으로) 실행된다** (Low / Confirmed)

### 데이터 손상·유실 가능성

없다. 저장 원자성·백업 폴백·파일 전용 필드 병합(`save_base`)·히스토그램 기준 태그가 모두 현행 코드에 살아 있고, 매 저장마다 라이브 카운터를 `take_live_stats` 한 쌍으로만 옮기는 규칙이 지켜진다. 새로 생긴 `iLastUpdateCheck` 쓰기 경로도 같은 `persist_with`를 타므로 규약을 깨지 않는다.

### 가장 먼저 수정할 영역

1. `src/debounce/thresholds.rs` — `key_threshold_ms(0x0A)`가 휠 실제 창(기본 80ms·`key.0A` 재지정)을 보여주도록 한다.
2. `src/ui/update.rs` — 업데이트 풍선 플래그에 유효기간을 둔다(표시 시점 기록 또는 다른 풍선 표시 시 해제).
3. `src/ui/update.rs` + `auto_tick` — 저장 실패 시 인메모리 백오프(다음 시도까지 대기)를 둔다.

---

## 2. Project Understanding

### 프로젝트 목적

Windows 저수준 훅(`WH_KEYBOARD_LL`, 선택적 `WH_MOUSE_LL`)으로 같은 키·버튼·휠의 **채터링(접점 튐) 재입력을 실시간 차단**하는 상주형 트레이 앱이다. 키별 기준·권장값 자동 계산·떼기 확인·마우스 버튼·휠 역스크롤·청소 모드·자동 일시 중지·서명 인앱 업데이트를 지원한다. 언어는 한국어·영어.

### 주요 entrypoint

| Entry | 위치 | 역할 |
|---|---|---|
| `main` | `src/main.rs` | `cli::handle_cli()` → `bootstrap::run(config::load())` |
| CLI | `src/app/cli.rs` | `--version`/`-V`, `--check-language`, `--check-helper`, `--check-update`, `--toggle`·`--pause-15`·`--pause-60`·`--resume`·`--cleaning` (실행 중 인스턴스로 `WM_COPYDATA` 전달) |
| GUI 기동 | `src/app/bootstrap.rs` | `init_shared` → (`--after-pid` 대기) → (자동 실행 시 관리자 인스턴스 대기) → 단일 인스턴스 → 훅 스레드 → 창 → 메시지 루프 |
| 창 프로시저 | `src/ui/window/proc.rs` `wndproc` | 500ms 타이머, `WM_COPYDATA` 제어 명령, 풍선 클릭, 세션 종료 저장, 업데이트 종료 |
| 키보드 훅 | `src/input/hook/procs.rs` `hook_proc` | 전용 스레드의 LL 콜백 |
| 마우스 훅 | `src/input/hook/procs.rs` `mouse_proc` | 버튼 + 휠(`WM_MOUSEWHEEL`) 분류, 같은 스레드 |
| 업데이트 헬퍼 | `crates/kcfix-updater` | 종료 대기 → 재해시 → 교체 → 재실행·준비 확인 → 실패 시 롤백 |

### 핵심 모듈

- `debounce` (`types`/`filter`/`thresholds`/`stats`): 시간 기반 판정 코어. `Decision::{Pass, Block, Defer, Flush}`, 휠은 `decide_wheel` 분리.
- `input::hook` (`procs`/`control`/`queue`/`send`): 훅 설치·분류·주입. `SendInput`은 인젝터 스레드 전용.
- `config`: `model`(`Settings`), `storage/`(원자적 INI 저장), `autostart/`(Run·작업 스케줄러), `sound`(경로 검증).
- `recommend`: 간격 히스토그램 → 키별 권장 기준(`Advice`), 순수 함수.
- `ui`: `settings_form`(폼·저장), `keys_page`(키별 기준 UI), `history`(기록·CSV), `stats`(홈 렌더·diff), `watchdog`(훅 감시), `foreground`(자동 일시 중지·관리자 창 감지), `elevation`(관리자 재실행·로그인 작업), `update`(수동+자동 확인 워커), `hotkeys`, `tray`, `files`, `menu`, `diag`(`%TEMP%/kcfix-diag.log`).
- `updater` + `crates/kcfix-updater`: 서명 매니페스트, 핀 고정 URL, 이중 해시, 스테이징·헬퍼·실행.

### 데이터 저장 방식

| 저장소 | 내용 | 쓰는 곳 |
|---|---|---|
| `%APPDATA%\KeyboardChatteringFix\settings.ini` (또는 exe 옆 포터블) | 설정 20여 개 + `key.XX` + `stat.XX` + `hist.XX`/`sHistMeasure` + `iLastUpdateCheck` | `config::save` ← `persist_with` 계열·`on_save`·가져오기 |
| `settings.ini.bak` / `.tmp` | 직전 세대 / 스테이징 | `save` 내부 |
| `HKCU\...\Run`, `Explorer\StartupApproved\Run` | 자동 실행 | `config::autostart` (토글 변경·파일 소실 때만 다시 씀) |
| 작업 스케줄러 `\KeyboardChatteringFix` + `HKCU\Software\KeyboardChatteringFix\ElevatedTask` | 관리자 로그인 시작과 표식 | `set_elevated_task`·`refresh_elevated_task` (COM 등록) |
| `%TEMP%` | 업데이트 스테이징, `kcfix-diag.log`(256KB 회전) | `updater::staging`, `ui::diag` |

DB는 없다.

### 외부 의존성

- **크레이트:** `windows 0.62`, `ureq 3` (TLS), `ed25519-dalek`, `sha2`, `serde_json`, `tracing`. 헬퍼 쪽 `clap`, `hex`, `thiserror`.
- **외부 서비스:** GitHub Releases 매니페스트·아티팩트 (핀 고정 URL + Ed25519 서명).
- **OS:** Windows 전용(설계 의도). LL 훅, `SendInput`, 레지스트리, 작업 스케줄러 COM, `ShellExecuteW`.

### 핵심 실행 흐름

```
[키 입력] OS → hook_proc(전용 스레드) → SHARED 락 → Debouncer::decide_event
          → apply: Pass(CallNextHookEx) / Block(LRESULT 1 + 사운드)
                   / Defer(pending_up + 스레드 타이머) → drain → injector 스레드 SendInput(INJECT_MARK)

[휠 입력] OS → mouse_proc(같은 스레드) → master 스위치 확인 → decide_wheel(방향 반전 + 80ms/key.0A)
          → Block(사운드 + LRESULT 1, last_wheel 미갱신) / Pass(방향 갱신)

[CLI 제어] 두 번째 실행 --toggle 등 → FindWindowW → WM_COPYDATA(dwData=allowlist) → wndproc → commands::run

[설정 저장] 저장 버튼 → on_save (disk=try_load 기준 + 폼 값, save_base 규약) → SHARED 적용 → config::save
[주기 저장] WM_TIMER 500ms → autosave_tick(5분) → persist_live → try_load로 key.XX 병합 → save
[자동 업데이트] WM_TIMER 500ms → auto_tick(due? uptime≥60s + 7일) → spawn_auto_check(워커)
          → check_for_update → note_update_checked → Available이면 트레이 풍선만 (클릭 시 수동 흐름)
[수동 업데이트] 메뉴/트레이 → spawn_update_check(워커) → 서명 검증 → 확인 대화상자
          → prepare_update(해시) → launch_helper → WM_UPDATE_EXIT(nonce) → 헬퍼 교체·재실행·롤백
[자동 실행] 로그인 → (Run) 일반 --autostart / (작업) 관리자 --autostart
          → 표식 있으면 일반 인스턴스가 관리자 인스턴스를 최대 20초 대기 → 단일 인스턴스
```

### 이전 감사 이슈 재검증 (2026-10-22 감사, v0.3.3 기준)

| 이전 이슈 | 현재 상태 | 근거 |
|---|---|---|
| ISSUE-001 줄 끝 주석 오해석 | 해소 유지 | `strip_inline_comment`(`storage/parse.rs:25`) + README 샘플 회귀 테스트 통과 |
| ISSUE-002 정전 후 설정·백업 유실 | 해소 유지 | `write_durably(sync_all)` + `MOVEFILE_WRITE_THROUGH` + torn 파일 `.bak` 폴백(`storage/io.rs`, `guard.rs`) + 테스트 통과 |
| ISSUE-003 관리자 경로 신뢰 경계 | 완화 유지 (잔여는 5장 Gap) | 관리자 인스턴스 인앱 업데이트 거부(`update_route` Elevated 분기), 쓰기 가능 위치 확인(기본 No), 타 계정 거부 — 코드·테스트로 확인 |
| ISSUE-004 관리자 창 전경 워치독 오탐 | 해소 유지 | `check(..., elevated_foreground)` + `foreground::status().1` 사용(`watchdog.rs:47,158`) + 테스트 통과 |
| ISSUE-005 `on_save` 규약 위반 | 해소 유지 | `save_base(config::try_load(), ...)`(`settings_form/save.rs`, `persist.rs:save_base`) + 테스트 통과 |
| 워치독 세대 인계 / 청소 모드 보류 뗌 / 스테이징 정리 / 해시 스트리밍 / `WM_UPDATE_EXIT` nonce | 해소 유지 | 해당 코드·테스트(`concurrent_restarts…`, `flush_deferred…`, `try_begin_update_parallel…`, `update_exit_needs_this_process_nonce` 등) 전부 통과 |

---

## 3. Audit Coverage & Limitations

### 실제 확인한 주요 모듈 (전체 열람)

- 입력·필터: `src/input/hook/{procs,control,queue,send}.rs`, `src/debounce/{filter,thresholds,types,stats}.rs`, `src/debounce/tests.rs`
- 설정·자동 실행: `src/config/storage/{parse,serialize,io,guard,encoding}.rs`, `src/config/{model,sound}.rs`, `src/config/autostart/{run,key,task}.rs`(서베이), `src/app/{bootstrap,instance,cli,state}.rs`
- UI: `src/ui/{update,commands,window/proc,window/handlers,settings_form/{save,persist,fields,sound},keys_page/{model,actions,table},history/{csv,rows},stats/update,foreground,watchdog,tray,menu,elevation(서베이),diag,ids,layout/{specs,query,page}}`
- 업데이트: `src/updater/{http,model,manifest(서베이),staging/guard}.rs`, `crates/kcfix-updater`(테스트명·CLI 호환 테스트로 확인)
- 문서·빌드: `README.md`(기능·설정 샘플·CLI 표), `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `CHANGELOG.md`(Unreleased), `docs/UPDATE.md`(동작 방식 절), `build.rs`, `bucket/`, `packaging/winget`, `scripts/e2e/README.md`, `tests/{version_sync,exe_smoke,edition_2024}.rs`

### 구조 분석 방법과 호출 관계

CodeGraph를 사용하지 않았다. `.codegraph/codegraph.db`는 저장소에 있으나, 이 실행 환경에는 CodeGraph MCP도 `codegraph` CLI도 없어(`Get-Command codegraph` 무응답, PATH에 없음) 호출 그래프를 조회할 수 없었다. 대신 `muse.search` + 파일 직접 열람으로 다음 관계를 추적했다.

- `decide_event` ← `hook_proc`·`mouse_proc`(버튼) / `decide_wheel` ← `mouse_proc`(휠 전용)
- `set_mouse_filter`·`set_wheel_filter` ← `persist`·`save`·`bootstrap` → `MOUSE_WANTED`/`WHEEL_WANTED` → 훅 스레드 `WM_KCFIX_MOUSE`
- `control_command`·`copydata_command`·`forward_control_to` ← `cli::handle_cli` / → `wndproc WM_COPYDATA` → `commands::run`
- `auto_tick` ← `wndproc WM_TIMER` → `spawn_auto_check` → `check_for_update` → `note_check_result` → `persist::note_update_checked` → `persist_with` → `config::save`
- `update_in_progress`·`try_begin_update`·`end_update` ← 수동/자동 워커 + 트레이/메뉴 busy 표시
- `take_live_stats`·`restore_live_stats` ← `persist_with`·`on_save`·`import_settings`·`init_shared`
- `foreground::tick` → `SHARED.suspended` / `status()` → `update_stats` + `watchdog::tick`
- `take_snapshot` (history/keys) → `SHARED` 단기 락 + 순수 렌더

### 실행한 테스트

```
cargo test --workspace   → 전 바이너리 ok (종료 코드 0)
  메인: 221 passed · 헬퍼 lib 4 · 헬퍼 main 2 · 헬퍼 통합 10
  edition_2024 4 · exe_smoke 5 · version_sync 6 · doctest 0
```

동일 트리에서 이번 세션에 `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`도 통과했다 (경고는 기존 manifest 관련 `warn` 2건뿐).

### 확인하지 못한 환경·외부 서비스

- 관리자 권한(UAC) 실기 경로: 관리자 인스턴스·무결성 수준 교차 `SendInput`·뮤텍스·로그인 작업 실기동. 감사 세션은 중간 무결성(비관리자)이다.
- 전원 차단·블루스크린 상황의 파일 시스템 동작, GitHub 실서버 업데이트 왕복, 실제 마모 휠 장치, 실제 풍선 클릭.
- GUI E2E 스크립트(`scripts/e2e/*.ps1`) 미실행 (프로젝트 규칙상 테스트에서 GUI를 띄우지 않으며, 릴리스 전 수동 절차다).
- Linux/macOS: Windows 전용 설계이므로 해당 없음.

### 분석의 한계

- 런타임 재현은 단위 테스트 수준까지만 했다. 훅 타이밍·스레드 경합·UAC 전환은 코드 근거 + 기존 테스트로 판단하고, 근거가 약한 것은 이슈로 올리지 않았다.
- CodeGraph 없이 수동 추적했으므로, 호출자 열거가 grep 수준에서 놓친 경로가 있을 수 있다. 핵심 핫패스·저장·업데이트 경로는 파일을 전부 열어 보완했다.

---

## 4. High-Risk Issues

심각도 Medium 이상에 해당하는 Confirmed/Likely 문제를 찾지 못했다. 아래 3건은 모두 신뢰도 Confirmed, 심각도 Low이며, 이슈 수를 채우기 위한 것이 아니라 실제 코드 결함이므로 기록한다.

### [ISSUE-001] [키·마우스] 탭이 Mouse Wheel의 차단 기준을 전역값으로 보여주지만 실제 필터는 80ms로 동작한다

- **위치:**
  - `src/debounce/thresholds.rs`: `key_threshold_ms` (`10-18`), `effective_threshold` (`73-80`)
  - `src/debounce/filter.rs`: `decide_wheel` (`114-144`), 특히 `129-132` (per-key 없으면 `WHEEL_REVERSE_MS`)
  - 표시: `src/ui/keys_page/table.rs` `take_snapshot` (`99-101`, 전 키에 `key_threshold_ms` 사용) → `model.rs` `render_rows` → `history::rows::threshold_cell`
- **우선순위:** Low
- **신뢰도:** Confirmed (코드 흐름)
- **문제:** `key.0A` 재지정이 없을 때 Mouse Wheel 행의 "차단 기준" 칸은 전역값(기본 35ms)을 보여준다. `is_mouse_button(0x0A)`가 거짓이라 `effective_threshold`가 전역으로 떨어지기 때문이다. 그러나 실제 차단 창은 `WHEEL_REVERSE_MS`(80ms)다. 사용자는 "휠은 35ms로 막힌다"고 읽는다.
- **발생 조건:** 휠 필터를 켜고 휠 틈이 막혀 Mouse Wheel 행이 목록에 나타났는데 `key.0A`를 따로 주지 않은 경우. 항상 재현된다.
- **영향:** 표시만 틀리다. 필터 동작 자체는 정확하고(빠른 역방향만 차단), `key.0A`를 주면 표시·동작이 일치한다. 오해가 생겨도 휠이 더 막히거나 덜 막히지 않는다.
- **근거:** `thresholds.rs:73-80`에 `0x0A` 분기가 없고, `filter.rs:129-132`는 `per_key[WHEEL_VK]` else 80ms다. `table.rs:100`은 예외 없이 `key_threshold_ms`를 쓴다.
- **반증 확인:**
  - `set_key_override(0x0A, …)` 경로는 표시·동작 모두 `per_key[0x0A]`를 써서 일치한다. 어긋나는 것은 기본값 표시뿐이다.
  - 마우스 버튼(`0x01/02/04-06`)은 `effective_threshold`가 마우스 기준을 돌려줘 표시·동작이 일치한다. 휠만 빠졌다.
  - `mouse_hint` 문자열에 "휠은 80ms 고정" 취지가 적혀 있어 완전한 오안내는 아니다. 그래도 숫자 칸 값 자체는 틀리다.
- **호출·영향 범위:** `decide_wheel` ← `mouse_proc`. 표시 ← `take_snapshot` → `render_rows`/`threshold_cell` → 기록 탭·CSV(`render_keys`도 같은 `KeyTotals.thresholds` 사용 — per-key 요약의 휠 행도 같은 값을 보여준다).
- **권장 수정 방향:** `key_threshold_ms` (또는 `effective_threshold`)에서 `vk == WHEEL_VK`일 때 per-key 없으면 80ms를 돌려준다. `decide_event`가 `0x0A`를 받는 일은 없으므로(예약 코드, 훅·버튼 매핑 모두 미사용) 판정 동작은 바뀌지 않는다.
- **필요한 회귀 테스트:**
  - `key_threshold_ms(0x0A) == (80, false)` (기본), `set_key_threshold(0x0A, Some(120))` 후 `(120, true)`, `Some(0)` 후 `(0, true)`.
  - 스냅샷 렌더: 재지정 없는 휠 행의 기준 칸이 `"80 ms"` (또는 스키마 문구에 맞춘 값)인지 `render_rows` 수준에서 확인.

### [ISSUE-002] 업데이트 풍선이 사라진 뒤에도 클릭 플래그가 남아, 이후 다른 풍선 클릭이 업데이트 흐름을 연다

- **위치:**
  - `src/ui/update.rs`: `UPDATE_BALLOON` (`55-59`), `spawn_auto_check` (`235-242`, 표시 전에 `true`), `take_update_balloon` (`57-59`)
  - `src/ui/window/proc.rs`: `WM_TRAY` → `NIN_BALLOONUSERCLICK` 분기 (`199-206`)
- **우선순위:** Low
- **신뢰도:** Confirmed (코드 흐름)
- **문제:** 플래그는 "업데이트 풍선을 보여줬다"는 사실만 기록하고, 풍선이 시간 초과로 사라지거나 다른 풍선(일시 중지 종료·청소 모드 자동 종료 등)으로 덮여도 해제되지 않는다. 그 뒤 사용자가 *어떤* 풍선이든 클릭하면 `take_update_balloon()`이 `true`를 돌려줘 업데이트 확인 대화상자가 열린다. `Shell_NotifyIconW` 실패 시에도 플래그는 이미 세워져 있다.
- **발생 조건:** 자동 확인이 새 버전을 알린 뒤 사용자가 풍선을 클릭하지 않고 놔뒀다가, 이후 다른 풍선을 클릭하는 경우.
- **영향:** 엉뚱한 타이밍에 업데이트 확인 대화상자가 열린다. 파괴적이지 않다(사용자가 취소하면 끝, 실제 업데이트는 확인을 눌러야 진행). 혼란 수준의 문제다.
- **근거:** `UPDATE_BALLOON`을 해제하는 곳은 `take_update_balloon`뿐이고, 호출자는 풍선 클릭 분기뿐이다. 타임아웃·교체 시 해제가 없다. `balloon()` 반환값은 무시된다(`let _ =`).
- **반증 확인:**
  - 다른 풍선들(`pause_ended`, `cleaning_auto_ended`, `notify_hidden_once`)은 플래그를 건드리지 않으므로, 업데이트 풍선 클릭과 구분할 방법이 코드에 없다.
  - 클릭 없이 풍선이 사라지는 것이 정상 흐름(Windows 풍선은 수 초 뒤 자동 소멸)이라, 플래그 잔류는 예외가 아니라 흔한 경우다.
- **호출·영향 범위:** `spawn_auto_check`(워커) → `tray::balloon` / `wndproc WM_TRAY` → `commands::run(MENU_UPDATE)`. 트레이·메뉴의 다른 동작에는 영향이 없다.
- **권장 수정 방향:** (a) 풍선 표시 시각을 함께 기록하고 클릭 시 수십 초 이내일 때만 수락, 또는 (b) 다른 풍선을 띄울 때 플래그를 해제(`balloon()` 진입 시 호출자가 아닌 중앙에서 관리), 또는 (c) `Shell_NotifyIconW` 실패 시 플래그를 세우지 않음. 셋 중 하나면 된다.
- **필요한 회귀 테스트:**
  - 순수 상태 기계로 분리 시: `show_update_balloon()` 후 N초 뒤 클릭은 무시, N초 안 클릭은 수락, 다른 풍선 표시 후 클릭은 무시.
  - 지금 구조에서는 HWND 없이 테스트 가능한 `should_open_update(flag_set_at, clicked_at)` 같은 순수 함수로 떼어 검사.

### [ISSUE-003] 설정 파일 저장이 계속 실패하면 자동 업데이트 검사가 사실상 연속 실행된다

- **위치:**
  - `src/ui/update.rs`: `auto_tick` (`252-270`, `due`면 매 틱 `spawn_auto_check`), `note_check_result` (`61-67`) → `persist::note_update_checked` → `persist_with` (실패 시 `false`, 호출자는 무시)
  - `src/ui/settings_form/persist.rs`: `note_update_checked` (`133-135`)
- **우선순위:** Low
- **신뢰도:** Confirmed (코드 흐름)
- **문제:** 다음 자동 검사 시각은 오직 `SAVED.last_update_check_unix` 하나로 정해진다. `config::save`가 실패하면(읽기 전용 매체 위 포터블 실행, 권한 문제 등) 스탬프가 전진하지 않으므로 `due`가 계속 참이다. 검사가 끝날 때마다(`end_update`) 다음 500ms 틱이 즉시 새 검사를 시작한다. 단일 비행 가드 덕에 동시 실행은 1개지만, 네트워크 검사가 사실상 쉬지 않고 반복된다.
- **발생 조건:** `settings.ini`에 쓸 수 없는 상태에서(읽기 전용 USB·공유 폴더의 포터블 실행이 대표적) 자동 확인이 켜져 있고(기본값) 네트워크는 되는 경우. 수동 확인도 스탬프를 남기지 못하므로 주기는 갱신되지 않는다.
- **영향:** 조용히 네트워크 검사(매니페스트 GET, 타임아웃 최대 90s)가 반복된다. UI 스팸·데이터 손상은 없다. 트래픽은 작지만(수 KB) 의도한 "주 1회"와 정반대 동작이다.
- **근거:** `note_update_checked` 반환값은 양쪽 호출자(`spawn_auto_check`, `spawn_update_check`) 모두 무시한다. `auto_tick`은 `update_in_progress()`만 보고, "연속 실패 중"이라는 기억을 어디에도 두지 않는다.
- **반증 확인:**
  - `try_begin_update` CAS가 동시 실행을 막는 것은 확인했다. 그러나 순차 반복은 막지 않는다. 검사가 빨리 실패할수록(예: DNS 즉시 실패 + 재시도 스탬프도 저장 실패) 반복 주기가 짧아진다.
  - 정상 환경(APPDATA 쓰기 가능)에서는 `save`가 성공해 스탬프가 전진하므로 발생하지 않는다. 조건이 좁아 Low다.
- **호출·영향 범위:** `auto_tick`(UI 타이머) → 워커 → `check_for_update`(ureq, 타임아웃 있음) → `config::save` 실패 반복. 훅·필터·UI 렌더에는 영향이 없다(워커 스레드, 락은 단기).
- **권장 수정 방향:** 저장 실패를 `auto_tick`이 알 수 있게 한다. 예: `note_update_checked`의 bool을 인메모리 "연속 실패 횟수"로 넘겨 실패 시 다음 시도를 1시간 뒤로 미루거나, `UPDATE_BALLOON`처럼 프로세스 내 `LAST_AUTO_ATTEMPT: Instant`를 두고 틱당 1회·최소 간격을 강제한다. 스탬프 파일 값과 무관하게 동작해야 한다.
- **필요한 회귀 테스트:**
  - `auto_tick`의 스폰 결정을 순수 함수로 분리해: "마지막 시도 N분 전 + 저장 실패 플래그" 입력이면 스폰하지 않음을 검사.
  - 저장 실패 시뮬레이션(읽기 전용 `APPDATA` 격리, 기존 `save_reports_failure_when_appdata_is_unusable` 패턴 재사용): `note_update_checked`가 거짓을 돌려주고, 그 뒤 `due`가 참이어도 스폰하지 않음을 검사.

---

## 5. Potential Functional Gaps

버그가 아니라 보완 여지다. 추정 사항을 버그처럼 쓰지 않는다.

- **Confirmed Gap — 수평 휠(`WM_MOUSEHWHEEL`)은 필터 대상이 아니다.** `wheel_delta`(`input/hook/procs.rs:106`)는 `WM_MOUSEWHEEL`에만 반응하고, 수평 틸트 스크롤은 분류 없이 통과한다. 수직 휠만 다룬다는 점이 README·힌트에 명시되어 있지 않다. 수평 휠 채터는 수직보다 드물어 영향은 작다.
- **Likely Gap — 쓰기 가능한 위치의 관리자 자동 실행은 사용자 동의 하나로 영속 승격 경로가 된다.** 기본 No 경고(`confirm_writable_location`, `elevation.rs:129`)·타 계정 거부·관리자 인앱 업데이트 거부가 있어 전보다 안전하다. 그래도 "그래도 켤까요?"에서 예를 누르면 매 로그인 관리자 권한으로 사용자 쓰기 가능 exe가 실행된다. 위협 모델 문서화는 되어 있으나(경고 문구), 처음 켤 때 Program Files 이전을 강제하지는 않는다.
- **추정 — 스위치 상태 추이·열화 알림(P2-2)이 없다.** 누적값만 있어 "최근 나빠졌는지"를 모른다. `docs/FEATURE_PROPOSALS.md`의 후속 항목이며, 히스토그램 영속화 토대(P0-1)는 갖춰졌다.
- **추정 — 앱별 프로필(앱마다 다른 기준, P3-2)이 없다.** 현재 앱별 동작은 일시 중지만이다. 게임에서 더블 탭 키를 쓰는 요구 등은 키별 끄기·시간 제한 일시 중지로 우회한다.
- **추정 — 키보드 배열 히트맵·키 테스트 화면(P3-1)이 없다.** 진단은 목록 기반이다.
- **추정 — 추가 언어(P3-3)·Per-Monitor DPI v2(P3-4)·코드 서명/winget 제출(P1-5, 외부 절차)이 남아 있다.** `packaging/winget/`은 준비됐고 미제출임이 README에 명시되어 있다.
- **추정 — PAC/WPAD 프록시는 미지원이다.** 수동 프록시는 지원(`http.rs:system_proxy`, PAC·우회 목록은 미적용이라 코드 주석에 명시). 기업망 일부에서 자동 확인·업데이트가 실패할 수 있다.
- **검토했으나 채택하지 않을 것 (기록용):** 장치별 필터(raw input 등록 시 전경 창에서 LL 훅이 입력을 못 받음, 실측), 커널 드라이버(서명·안티치트 부담), 권장값 자동 적용(확인 없는 변경 위험). `docs/FEATURE_PROPOSALS.md` 판단과 일치한다.

---

## 6. Documentation Mismatches

- **`GEMINI.md` — 필터 코어 경로가 낡았다.** "Filter core: `src/debounce.rs`"라고 적혀 있으나 현재는 `src/debounce/` 디렉토리(`filter`·`thresholds`·`types`·`stats`)다.
- **`GEMINI.md` — 키별 기준 설명이 낡았다.** "Per-key thresholds (`key.XX`) are config-file only"라고 적혀 있으나 현재는 [키·마우스] 탭에서 즉시 적용·저장된다 (v0.3.0~). 같은 줄의 "Save applies file edits live"는 여전히 맞다.
- **`docs/UPDATE.md` — 자동 주간 확인 언급이 없다.** 동작 방식 절이 수동 확인(메뉴·`--check-update`)만 설명한다. 새 풍선·클릭 흐름·`bAutoCheckUpdate`/`iLastUpdateCheck`에 대한 문장 하나가 빠져 있다. 릴리스 절차 자체는 그대로라 심각하지 않다.
- **없음을 확인한 것:** README 설정 샘플(영문·국문)은 파서와 일치한다(회귀 테스트가 강제). 임계값 기본값(35/30/80ms)·CLI 표(제어 플래그 포함)·관리자 한계·포터블 모드 서술은 코드와 일치한다. `CLAUDE.md`의 창 높이(716)·저장 행(640)·카드 상한(628)·설정 쓰기 목록(`note_update_checked` 포함)은 현행 코드와 일치한다.

---

## 7. Recommended Fix Plan

실제 코드는 수정하지 않았다. 아래는 순서 제안이다.

### Phase 1 — Immediate (이번 릴리스)

- **ISSUE-001:** `key_threshold_ms`에서 `WHEEL_VK`를 휠 실제 창(기본 `WHEEL_REVERSE_MS`, `key.0A` 재지정 시 그 값)으로 매핑한다. 판정 코드는 그대로다. 표시 회귀 테스트와 함께.
- **ISSUE-002:** 풍선 플래그에 유효기간을 둔다(표시 시각 기록 또는 타 풍선 표시 시 해제). 순수 함수 분리 + 단위 테스트.
- **ISSUE-003:** 저장 실패 시 인메모리 백오프(다음 시도까지 최소 간격)를 둔다. `note_update_checked`의 반환을 `auto_tick` 결정에 반영한다.

### Phase 2 — Stability

- 수평 휠 처리 여부 결정(막기 또는 "수직만" 문서화). 결정 후 `wheel_delta` 테스트 확장.
- `docs/UPDATE.md`에 자동 확인 한 절 추가(주기·풍선·클릭·수동 확인과의 관계).
- 관리자 자동 실행: Program Files 이전 안내를 경고보다 강하게(기본 거부 유지 + 문서 링크) 할지 결정. 위협 모델은 이미 경고문에 있다.
- 신규 기능 e2e 추가(8장 참고).

### Phase 3 — Structural

- P2-2 스위치 추이(일별 증분 저장), P3-2 앱별 프로필, P3-1 히트맵, P3-3 언어, P3-4 Per-Monitor DPI, P1-5 서명·winget 제출.
- `SAVED`/`SHARED` 락 순서를 코드 주석으로 고정한다(현재 SAVED→SHARED 한 방향만 존재함을 명시 — 이번 감사에서 역방향 중첩을 찾지 못했으나, 후속 작업자가 깨뜨리지 않도록).

---

## 8. Test Recommendations

- **Unit**
  - (ISSUE-001) `key_threshold_ms(0x0A) == (80, false)` 기본, `set_key_threshold(0x0A, Some(200))` 후 `(200, true)`, `Some(0)` 후 `(0, true)`. 스냅샷 렌더 수준에서 재지정 없는 휠 행 기준 칸이 `"80 ms"`인지 확인.
  - (ISSUE-002) `show_update_balloon()`/`balloon_clicked(now)` 순수 분리: 표시 후 N초 안 클릭만 수락, 다른 풍선 표시 후 클릭은 무시.
  - (ISSUE-003) `note_update_checked` 실패를 `auto_tick` 결정이 반영: "마지막 시도 5분 전 + 저장 실패" 입력이면 스폰하지 않음. 기존 `save_reports_failure_when_appdata_is_unusable` 격리 패턴 재사용.
  - (기존 강점 유지) `decide_wheel` 경계(정확히 80ms 통과)·차단 후 방향 유지·`key.0A=0` 무필터, `copydata_command` allowlist, `auto_check_due`·`fail_retry_stamp` 경계 — 이미 존재하므로 깨뜨리지 말 것.
- **Integration**
  - 격리 APPDATA에서 `bAutoCheckUpdate=0` 저장→로드 왕복, `iLastUpdateCheck` 큰 값·0 왕복 (일부는 존재, `filter_wheel` 포함 전체 필드 왕복은 존재 — 유지).
  - `on_save`가 `last_update_check_unix`를 보존하는지 (디스크 값 유지 + 토글만 반영).
  - 가져오기(`import_settings` 경로): 가져온 파일의 `bAutoCheckUpdate`가 폼·라이브에 반영되는지.
- **End-to-End (수동, 릴리스 전 — `scripts/e2e` 확장 제안)**
  - 휠: `SendInput`(`MOUSEEVENTF_WHEEL`, INJECT 표식 없이)으로 반대 방향 틱을 10ms 간격으로 보내 차단 카운터가 오르고, 같은 방향 고속 스크롤은 전부 통과하는지. 훅 테스트는 합성 입력을 쓰지 않으므로(기존 주석) e2e가 유일한 실증 수단이다.
  - CLI: 테스트 빌드 + 격리 APPDATA에서 실행 중 인스턴스에 `--toggle` 전달 시 보호 토글 + `--pause-15` 후 상태 알약 "일시 멈춤" 확인. **주의: 전역 단일 인스턴스 뮤텍스를 쓰므로 실제 상용 앱이 떠 있으면 먼저 끌 것.**
  - 자동 확인: `iLastUpdateCheck=0` + `bAutoCheckUpdate=1` 상태에서 기동 60초 뒤 네트워크 없이(방화벽 차단) 조용함(다이얼로그 없음)을 확인하고, 모의 `update.json`은 코드 변경 없이 불가하므로(핀 고정 URL) 이 부분은 `check_for_update` 단위 수준 + 클릭-수동 흐름 E2E로 분리. 향후 `MANIFEST_URL` 환경변수 오버라이드(`KCFIX_LOCALE` 패턴)를 두면 e2e가 가능해진다.
  - 기존 `hook_session_e2e`·`features_e2e`·`update_swap_e2e`·`language_e2e` 유지 실행.
- **Concurrency**
  - 자동 확인 워커의 `persist_with`와 저장 버튼 `on_save`가 같은 틱에 겹쳐도 `last_update_check_unix`가 단조 유지되는지 (스탬프 후퇴 허용 여부 결정 후 고정).
  - 기존 `concurrent_restarts_leave_one_hook_thread`·`try_begin_update_parallel_exactly_one_winner` 유지.
- **Regression**
  - 이번 감사에서 고치는 3건마다 위 단위 테스트를 고정한다. 기존 범위형 assertion은 정확한 값으로 좁히는 프로젝트 철칙을 따른다.
  - `readme_sample_parses_to_what_it_documents`에 새 키(`bAutoCheckUpdate`, `iLastUpdateCheck`) 단언 유지 (현재 존재).
- **Platform-specific**
  - Windows 전용. DPI 125%/150%에서 새 716 높이·이동한 저장 행·바탕이 겹치지 않는지 수동 확인. 다크·라이트 테마의 새 토글(`auto_update`) 가시성. 관리자·비관리자 교차 CLI 전달(거부 메시지). CP949 손 편집 파일의 신규 키 폴백.

---

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Good | 필터 코어·훅·업데이트 검증이 정확하다. 남은 것은 휠 기준 표시(ISSUE-001) 같은 표면 문제뿐이다 |
| Runtime Stability | Good | 훅 전용 스레드, 세대 인계, poison 내성, 락 밖 Win32 호출, 재진입 가드. 자동 확인 워커도 단일 비행 + 타임아웃으로 묶여 있다 |
| Data Integrity | Good | 원자 저장·백업·손상 폴백·파일 전용 필드 병합이 intact하고, 신규 필드도 같은 경로를 탄다 |
| Error Resilience | Good | 저장·자동 실행·작업·업데이트·헬퍼 실패마다 알림·롤백·재시도가 있다. ISSUE-003의 저장-실패 폭주가 유일한 예외 처리 빈틈이다 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도이며 문서와 일치한다. PAC 프록시·관리자 교차 경로는 문서화된 한계다 |
| Test Confidence | Good | 252건 통과, 경계·폴백·동시성 테스트가 풍부하다. 다만 휠·CLI·자동 확인의 e2e와 관리자 실기는 자동 검증 밖이다 |

**실제로 먼저 수정할 문제 3개**

1. **ISSUE-001:** 휠 차단 기준 표시를 실제 창(80ms/`key.0A`)과 일치시킨다. 가장 눈에 띄는 불일치다.
2. **ISSUE-003:** 저장 실패 시 자동 검사가 연속 실행되는 것을 인메모리 백오프로 막는다. 유일한 예외 처리 빈틈이다.
3. **ISSUE-002:** 업데이트 풍선 클릭 플래그에 유효기간을 둔다. 엉뚱한 타이밍의 대화상자를 막는다.
