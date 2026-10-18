# Project Audit

- 대상: `keyboard-chattering-fix` (`kcfix-rs`) — 메인 바이너리 + `crates/kcfix-updater` 헬퍼 (Cargo 워크스페이스)
- 기준 시점: 2026-10-18, HEAD `0547ae2` (v0.3.0 릴리스 이후, 코드 변경 없음 — `git status`상 기존 미커밋 삭제분 `PROJECT_AUDIT_RELEASE.md`·`PROJECT_AUDIT_UI_TESTS.md` 2건은 본 감사와 무관하므로 손대지 않음)
- 방법: 문서 확인 → CodeGraph 탐색(10회) → 소스 직접 열람 → 테스트·게이트 실행. **코드는 수정하지 않았다.**
- 이 문서는 2026-10-17 작성의 이전 `PROJECT_AUDIT.md`(커밋 `a280cad` 수록)를 대체한다. 이전 문서의 ISSUE-001~005가 코드에서 해소됐음을 재확인했으며(아래 "이전 감사 이슈 해소 검증" 참조), 이번 감사는 그 이후 추가된 v0.3.0 기능(마우스 필터, 떼기 확인 `iReleaseDeferMs`, 워치독 프로브, 키·마우스 탭, 승격 작업 등)을 중심으로 다시 감사했다.

---

## 1. Executive Summary

**전체 상태.** 필터 코어(`Debouncer::decide_event`), 설정 원자 저장, 서명 업데이트 파이프라인은 정확하고 테스트도 촘촘하다.
`cargo test --workspace` 188개 전부 통과, `cargo fmt --check`·`cargo clippy -D warnings` 모두 깨끗하다.
이전 감사(2026-10-17)의 5개 이슈는 모두 코드에서 해소된 것을 확인했다.
이번에 새로 발견된 문제는 영향이 제한적이거나 발생 조건이 좁아, 과장하지 않고 기록한다.

**전체 위험도: Low–Medium.** Critical/High에 해당하는 확정 문제는 없다.
데이터를 파괴하는 경로는 없으며(원자 저장 + `.bak` 폴백 + 이중 해시 검증 유지),
남은 문제는 통계 부풀리기·일시적 필터 동작 어긋남·좁은 조건의 업데이트 실패 수준이다.

**가장 중요한 문제 3~5개**

1. **[ISSUE-001] 워치독 오판단 시 재시작 과정에서 두 훅 스레드가 잠시 공존** — 프로브 왕복이 1초를 넘기면(고부하) 살아 있는 훅을 버리고 새 스레드를 띄운다. 정상 경로에서는 `WM_QUIT`으로 깔끔히 인계되지만, 인계 틈새에 두 `WH_KEYBOARD_LL` 훅이 같은 `SHARED`를 공유하면 통계가 이중 집계되고 떼기 확인 상태가 어긋난다. (Medium / Likely)
2. **[ISSUE-002] 청소 모드가 진행 중인 떼기 확인(`pending_up`)을 고려하지 않음** — 청소 시작 직전 보류된 뗌이 타이머에 의해 청소 중에 방출되고, 청소 중 눌림은 보류분을 정리하지 않는다. 앱이 보지 못한 키의 뗌을 보는 수준의 어긋남이다. (Low / Likely)
3. **[ISSUE-003] `bMinimizeOnLaunch`만 불리언 규칙이 다름** — 다른 플래그는 "0만 거짓"인데 이 항목만 `== "1"` 비교라, 직접 편집한 파일에서 `yes` 같은 값이 보호 관련 플래그와 반대로 해석된다. (Low / Confirmed)
4. **[ISSUE-004] 시작 시 스테이징 청소가 진행 중인 업데이트 파일을 지울 수 있음** — 헬퍼가 교체 대기(최대 120초) 중 사용자가 앱을 재실행하면 새 프로세스의 `cleanup_stale_staging`이 잠겨 있지 않은 교체 파일을 삭제하고, 헬퍼의 사전 교체 해시 검사가 실패해 업데이트가 깨진다. (Low / Likely)
5. **관측 가능성 공백** — GUI 빌드(`windows_subsystem = "windows"`)에는 콘솔이 없어 `tracing` 진단(느린 틱 로그·업데이트 경로 로그)이 실제로는 버려진다. 과거 응답없음 리포트의 원인 추적 수단이 없는 상태다. (구조적 Gap, 5장 참조)

**데이터 손상/유실 가능성 여부.** 설정 파일 손상 경로는 없다(`settings.ini.tmp` 기록 → `.bak` 보관 → rename, 깨진 본 파일은 백업으로 폴백 — `src/config/storage.rs:213-271`, `70-81`). 유실 가능성은 (a) 크래시 시 최대 5분치 통계(의도된 설계, `autosave_tick`), (b) ISSUE-004의 교체 파일 삭제(복구: 업데이트 재시도, 이전 버전은 계속 실행 중)뿐이다. 사용자 데이터·시스템 손상은 없다.

**가장 먼저 수정해야 할 영역.** `src/input/hook.rs` 재시작 인계(구 스레드 종료 확인 후 신 스레드 기동, 또는 세대 번호로 구 스레드 방출 무효화) → `src/debounce.rs`·`hook.rs drain`의 청소 모드 가드 → 관측용 파일 로그(선택).

### 이전 감사 이슈 해소 검증 (2026-10-17 → 이번 감사)

| 이전 이슈 | 현재 상태 | 근거 |
|---|---|---|
| 자동 업데이트가 2회차부터 영구 실패 (헬퍼 엄격 버전 일치) | 해소됨 | `helper_compatible`이 `[HELPER_MIN_VERSION, app_version]` 범위를 허용 (`src/updater/staging.rs:150-153`), 관련 테스트 2건 통과 |
| 훅이 UI 스레드에서 실행 | 해소됨 | 훅은 전용 메시지펌프 스레드 (`spawn_hook_thread`, `src/input/hook.rs:499-577`), UI 정지와 무관 |
| 업데이트 "적용 중" 모달이 헬퍼 대기를 갉아먹음 | 해소됨 | 모달 없이 즉시 `WM_UPDATE_EXIT` 게시 (`src/ui/update.rs:123-129`), 헬퍼 대기 120초 (`HELPER_EXIT_TIMEOUT_SECS`) |
| OS 종료 시 미저장 | 해소됨 | `WM_ENDSESSION(TRUE)`에서 `persist_live` + 트레이 제거 (`src/ui/window.rs:575-585`), 종료 veto 없음 |
| 실행 중 파일 편집이 스냅샷으로 덮어씀 | 해소됨 | `adopt_file_only_fields` + 디스크 재읽기 병합 (`persist_with` `src/ui/settings_form.rs:239-267`, `on_save`는 디스크를 기준으로 시작 `440`) |

---

## 2. Project Understanding

### 프로젝트 목적

Windows 저수준 키보드 훅(`WH_KEYBOARD_LL`)으로 기계식 스위치 바운스(채터링)를 가로채 버리는 상주형 필터다.
관리자 권한·DLL 인젝션 없이 동작하고, Win32 네이티브 UI(홈·기록·키·마우스·설정·고급 탭, 트레이)를 쓴다.
청소 모드, 키별 임계값·권장값, 마우스 버튼 필터, 떼기 확인(릴리스 지연), 자동 일시 중지, Ed25519 서명 기반 업데이트를 제공한다.
설계상 Windows 전용이다.

### 주요 entrypoint

| Entrypoint | 위치 | 역할 |
|---|---|---|
| `main` | `src/main.rs:30` | `app::cli::handle_cli()`(`--version`/`--check-language`/`--check-helper`/`--check-update`) 후 `app::bootstrap::run(config::load())` |
| `bootstrap::run` | `src/app/bootstrap.rs:76` | `init_shared` → `--after-pid` 대기 → 단일 인스턴스 뮤텍스 → 핸드셰이크 ack → 훅 스레드 기동 → 창 생성 → 메시지 루프 → 종료 시 `hook::stop` |
| `hook_proc` / `mouse_proc` | `src/input/hook.rs:164` / `245` | OS가 호출하는 필터 콜백. 훅 전용 스레드에서 실행. 락 + `decide_event` + `apply`만 수행 |
| `wndproc` | `src/ui/window.rs:486` | 500ms 타이머·명령·트레이·세션 종료·닫기 라우팅 |
| 헬퍼 `main` | `crates/kcfix-updater/src/main.rs:59` | 대기 → 해시 재검증 → 교체 → 재실행·준비 확인 → 실패 시 롤백·이전 버전 재실행 |

### 핵심 모듈

| 모듈 | 역할 |
|---|---|
| `src/debounce.rs` | `Debouncer::decide_event` 상태 머신. 물리 슬롯(vk+확장, 512칸)으로 타이밍, vk(256)로 통계·임계값. `Decision::{Pass, Block, Defer, Flush}` |
| `src/app/state.rs` | `SHARED: OnceLock<Mutex<Shared>>`(훅·UI 공유), `START`(단조 시계), `HOOK_ACTIVE` |
| `src/input/hook.rs` | 훅 스레드 수명(`start`/`stop`/`restart`), `InjectQueue`(고정 32칸), injector 스레드(`SendInput` 전담), 떼기 확인 타이머, 워치독 프로브 응답 |
| `src/config/` | `Settings` 모델, INI 원자 저장(`storage`), Run 인용 경로·스케줄 작업(`autostart`), 사운드 사전 검증(`sound`) |
| `src/ui/` | `window`(디스패치·종료), `settings_form`(저장·자동저장), `stats`/`history`(스냅샷→렌더→diff), `foreground`(일시 중지·승격 감지), `watchdog`(프로브→재시작), `update`(워커), `files`(가져오기·내보내기), `keys_page`, `hotkeys`, `elevation` |
| `src/recommend.rs` | 간격 히스토그램 → `Advice::{NeedMoreData, NoChatter, Suggest, Overlap}` 순수 함수 |
| `src/updater/` + 헬퍼 | 매니페스트 서명·URL 핀·크기·만료·버전 검증, 다운로드·해시, 헬퍼 스테이징·실행·청소 |

### 데이터 저장 방식

- `%APPDATA%\KeyboardChatteringFix\settings.ini` (레거시 `KcfixRs` 폴백, exe 옆 `settings.ini`가 있으면 휴대 모드). 저장은 tmp 기록 → 기존 파일 `.bak` 복사 → rename. 본 파일이 비면 `.bak` 폴백.
- 쓰기 시점은 `on_save`(Save/가져오기), `persist_live`(종료·세션 종료·5분 자동저장·통계 초기화·언어 변경), 가져오기의 직접 저장 1회(`files.rs:141`, 아래 6장 참조)뿐이다.
- `HKCU\...\Run\KeyboardChatteringFix`(인용 경로 + `--autostart`), 승격 시작은 스케줄 작업(배터리·시간제한 해제 정의).
- DB·서버 상태 없음. 네트워크는 GitHub Releases 매니페스트·아티팩트 다운로드뿐(connect 30s / read 60s / total 90s).

### 외부 의존성

`windows 0.62`, `ureq 3`, `ed25519-dalek 3`, `sha2`, `serde_json`, `thiserror`, `tracing`(+subscriber), 헬퍼 쪽 `clap`·`hex` 등. 시스템 의존: LL 훅, `SendInput`, 레지스트리 Run, `schtasks`, `ShellExecuteW`.

### 핵심 실행 흐름

`입력 → 검증 → 핵심 로직 → 상태 변경 → 파일/DB/API → 결과/오류 처리` 형태로 정리하면 다음과 같다.

- **키 입력:** `OS 키 이벤트 → hook_proc(훅 스레드) → INJECT_MARK·청소·주입분류 선통과 판정 → SHARED.lock → Debouncer::decide_event → apply(Pass=통과, Block=차단+캐시 사운드, Defer=보류+타이머, Flush=주입큐+injector 기상) → LRESULT(1) 차단 / CallNextHookEx 통과`
- **떼기 확인:** `Up(Defer, pending_up 기록) → 훅 스레드 타이머 → drain(due_releases 만료분) → injector 스레드 SendInput(INJECT_MARK, 훅은 통과) → 바운스 다운이면 취소·차단, 늦으면 Flush(뗌 먼저·눌림 재심)`
- **설정 저장:** `Save → on_save(폼 파싱·클램프, 실패 시 실행값 폴백) → set_autostart → 디스크 재읽기를 기준으로 병합 → SHARED 적용 → 사운드는 워커 검증(세대 번호) → 마우스훅·전경·핫키 적용 → config::save → 실패 시 경고 대화상자`
- **통계 표시:** `WM_TIMER(500ms, 재진입 가드) → 짧은 락 스냅샷 → render → diff → 변경분만 SetWindowTextW. 기록 탭은 보일 때·log_seq 변경 때만`
- **워치독:** `10초간 키보드 무소식 + 사용자 활동(GetLastInputInfo) → SendInput 프로브(표식 VK 0xFF 키업) → 1초 내 미감지 → hook::restart → 성공 시 Home에 시각 기록`
- **업데이트:** `워커 스레드 → check_for_update(서명·핀고정 URL·크기·만료·신버전) → 패키지관리자 설치면 안내만 → 확인 → prepare_update(헬퍼해석→%TEMP% 스테이징→다운로드·크기·해시) → launch_helper → 즉시 WM_UPDATE_EXIT·종료 → 헬퍼: pid 대기(120s) → 해시 재검증 → 교체 → 재실행·준비 확인(60s) → 실패 시 롤백·이전 버전 재실행`
- **종료:** `WM_CLOSE(트레이 상주 or 파괴) / WM_UPDATE_EXIT / WM_ENDSESSION(TRUE, 즉시 persist) → WM_DESTROY(KillTimer·핫키 해제·persist_live·트레이 제거) → 메시지 루프 종료 → hook::stop(보류 뗌 전부 방출·join)`

---

## 3. Audit Coverage & Limitations

### 실제 확인한 주요 모듈

직접 열람: `main.rs`, `app/{bootstrap,cli,instance,state}.rs`, `input/hook.rs`(대부분)·`input/sound.rs`,
`debounce.rs`(상태머신 본체 `284-427` + 테스트 일부), `config/{model,storage,sound,autostart}.rs`,
`ui/{window(종료·타이머 경로),settings_form(저장 본체),watchdog,foreground(상부),files,history(csv·스냅샷),update,keys_page(적용 경로),state,hotkeys,elevation}.rs`,
`recommend.rs`, `updater/{model,manifest(검증 본체),http,staging(스테이징·헬퍼·청소),error}.rs`,
`crates/kcfix-updater/src/main.rs`(진입·실패 처리). 이전 감사 문서(2026-10-17)의 코드 인용과 대조해 변경분 위주로 재확인했다.

### CodeGraph로 분석한 호출 관계

`codegraph_explore` 10회로 다음 호출 경로를 확인했다(반환된 원문 소스를 기판독으로 간주, 부족분만 grep·직접 열람으로 보완).

- 진입·상태: `main → cli → bootstrap::run → init_shared`, `SHARED` 사용처(`hook.rs`·`bootstrap.rs`)
- 필터 코어: `decide_event ← hook_proc/mouse_proc`, `Decision` 분기, `due_releases/has_pending_releases ← drain`
- 훅 수명: `start/stop/restart/abandon/spawn_hook_thread`, `set_mouse_filter → 스레드 메시지`, `injector`·`drain`·`INJECT_MARK`·`PROBE_VK`
- 설정: `persist_live → persist_with → adopt_file_only_fields`, `config::save` 호출점 3곳, `try_load` 폴백
- 업데이트: `prepare_update/launch_helper/discard_staged/cleanup_stale_staging`, `download_and_verify`, `UpdateErrorKind` 사용처
- UI: `on_save` 본체, `apply_sound_async` 세대 번호, `session_reply/on_destroy`, `csv_snapshot`, `set_key_override`, `hotkeys::apply`, 단일 인스턴스·승격 경로

### 실행한 테스트

- `cargo test --workspace` (exit 0): 메인 162 + 헬퍼 3·2·10 + `exe_smoke` 5 + `version_sync` 6, 총 188개 전부 통과. 실패 0.
- `cargo fmt --all -- --check`: 통과. `cargo clippy --all-targets --all-features -- -D warnings`: 경고 0.
- 테스트에 사용된 CodeGraph 미사용 주장 없음, 실행하지 않은 테스트를 실행했다고 쓰지 않음(아래 미확인 항목 참조).

### 확인하지 못한 환경/외부 서비스

- 실기 GUI 기동·실제 키보드 채터링·E2E 스크립트(`scripts/e2e/`, 실바이너리·데스크톱·네트워크 필요)는 실행하지 않았다. `hook_runs_on_a_dedicated_thread_and_stops` 같은 환경 의존 테스트는 스위트 내에서 통과했다.
- GitHub Releases 도달성·실제 업데이트 왕복, `schtasks`/Run 실기 등록, UAC·관리자 창·전체화면·IME·원격 데스크톱 조합은 코드·테스트로만 확인했다.
- Linux/macOS 빌드: 설계상 Windows 전용이라 시도하지 않았다(아래 9장 참조).
- `AGENTS.md`는 저장소에 존재하지 않아(루트·추적 파일 목록 확인) "확인 문서"에서 제외했다. 지시문이 요구한 문서는 README·CLAUDE·GEMINI·Cargo 매니페스트·빌드 스크립트로 대체했다.

### CodeGraph 또는 분석상의 한계

- CodeGraph는 심볼 단위 호출 관계 파악에 썼고, 줄 번호·타이밍·Win32 의미 해석은 직접 열람으로 검증했다. UI 페인트·레이아웃 좌표 전수, `strings.rs` 전 문구 대조, 릴리스 스크립트·워크플로 전수는 이번 범위에 넣지 않았다(버전 동기화 테스트 6건이 파이프라인 드리프트를 대신 보증).
- 레이스·타이밍 이슈는 정적 근거 + 단위 테스트로만 판단하고 런타임 재현은 하지 않았다. 그래서 해당 이슈는 `Likely` 이하로만 분류했다.

---

## 4. High-Risk Issues

High-Risk에는 `Confirmed`와 근거가 강한 `Likely`만 넣는다. 이번 감사에서 Critical/High에 해당하는 확정 문제는 없었다.
아래 4건이 전부이며, 과장하지 않기 위해 Medium 1건·Low 3건으로 기록한다.

### [ISSUE-001] 워치독 오판단 시 두 훅 스레드가 잠시 공존하며 통계를 이중 집계한다

- **위치:** `src/input/hook.rs` — `restart` (`454-460`), `spawn_hook_thread` (`499-577`), `HookThread::abandon` (`491-493`); `src/ui/watchdog.rs` — `tick` (`109-151`), `send_probe` (`92-106`)
- **우선순위:** Medium
- **신뢰도:** Likely
- **문제:** 프로브(`SendInput` 표식 키업) 왕복이 `PROBE_TICKS`(2틱 = 1초)를 넘기면 워치독은 훅이 죽었다고 보고 `restart()`한다. `restart`는 구 스레드를 기다리지 않고(`abandon` = `WM_QUIT` 게시만) 새 스레드를 띄운다. 구 스레드가 살아서 메시지를 펌프 중이면 `WM_QUIT`을 받아 스스로 언훅·종료하므로 정상 인계되지만, **새 훅 설치 → 구 스레드의 `WM_QUIT` 처리 사이 틈새에는 두 개의 `WH_KEYBOARD_LL` 훅이 같은 `SHARED`를 공유**한다. 그 틈새의 키 이벤트는 두 훅이 순차 처리한다. 첫 훅이 상태 전이를 소비한 뒤 두 번째 훅이 전이 후 상태를 보므로, 같은 물리 눌림이 `total_presses`·`per_key_presses`에 **2회 집계**되고 알림음이 이중 재생될 수 있다. 떼기 확인 사용 중이면 구 스레드 종료 시 `send(&drain(true))`가 보류 중인 뗌을 조기 방출해 홀드 착각이 깨진다.
- **발생 조건:** (a) 마우스만 10초 이상 쓰고 키보드는 가만히 있는 일상적 상황에서 프로브가 나가고, (b) 그 왕복이 1초를 초과(극심한 부하·입력 스톨)하거나 `SendInput`이 실패(`let _ =`로 무시됨)할 때. (a)는 흔하고 (b)는 드물어 전체 확률은 낮다.
- **영향:** 통계 부풀리기(차단율·문제 키 진단 왜곡), 이중 알림음, 떼기 확인 중 한순간 홀드 해제. 키 stuck(눌림 고착) 방향은 아니다. 스레드 자체는 `WM_QUIT`으로 종료되므로 누수는 일시적이다.
- **근거:** `tick`의 실패 판정(`116-125`), `restart`의 무대기 교체(`455-460`), `hook_proc`의 공유 상태 순차 처리(`183-215`), 종료 시 `drain(true)` 방출(`562-564`). 이중 집계 메커니즘은 `judge_down`이 매 호출에서 카운터를 올리는 구조(`357-358`)에서 직접 도출된다.
- **반증 확인:** 상위 보호로 (1) 프로브는 `INJECT_MARK`라 일시 중지·주입 제외 상태에서도 훅이 삼키고(`176-182`), (2) 구 스레드가 펌프 중이면 반드시 `WM_QUIT`을 처리해 언훅하므로 이중 상태가 영속하지 않으며, (3) `stop()` 경로는 join으로 깔끔히 닫힌다. 그래서 영속적 이중 훅·영구 통계 오염은 성립하지 않고, **과도기 틈새(ms 단위, 키 입력이 겹칠 때만)** 로 한정된다. `try_acquire_single_instance`의 fail-open(`instance.rs:54-55`)은 이론상 같은 현상을 만들지만 `CreateMutexW` 실패는 사실상 발생하지 않아 제외했다.
- **호출/영향 범위:** caller `watchdog::tick ← wndproc WM_TIMER`; callee `restart → start → spawn_hook_thread`, `HookThread::quit`. 영향 모듈: `debounce` 통계, `sound` 재생, `stats`·`history` 표시, 떼기 확인(`drain`·`inject`).
- **권장 수정 방향:** 재시작 전에 `HOOK_THREAD_ID`의 구 스레드 종료를 짧게(예: 수백 ms) join 시도하고, 실패 시에만 포기 후 기동. 또는 훅 스레드에 세대 번호를 두고 구 스레드의 `drain(true)`·주입이 현 세대가 아니면 버리게 한다. `send_probe`의 `SendInput` 반환값을 확인해 전송 실패 시에는 재시작 대신 재프로브하도록 한다.
- **필요한 회귀 테스트:** (Unit) `probe_failed` 경계는 이미 존재. 추가: 세대 가드가 있는 `drain`이 구 세대 호출을 무시하는 테스트. (Integration) `restart()` 직후 `HOOK_THREAD_ID`가 정확히 1개 스레드를 가리키고 구 스레드가 N초 내 종료됨을 assert(데스크톱 있는 환경에서만).

### [ISSUE-002] 청소 모드가 진행 중인 떼기 확인을 고려하지 않는다

- **위치:** `src/debounce.rs` — `decide_event` 청소 분기 (`296-304`); `src/input/hook.rs` — `drain` (`380-406`), `hook_proc` 청소 분기 (`192-200`)
- **우선순위:** Low
- **신뢰도:** Likely
- **문제:** 청소 모드는 눌림 차단·뗌 통과만 결정하고 `pending_up`(보류 중인 뗌)을 건드리지 않는다. (a) 청소 시작 직전 보류된 뗌은 훅 스레드 타이머가 그대로 `drain`해 청소 중에 방출된다 — 앱은 본 적 없는(또는 청소 전) 눌림의 뗌을 뒤늦게 받는다. (b) 청소 중 눌림은 `pending_up`을 정리하지 않으므로, 청소 종료 후 첫 눌림이 `Flush` 경로를 탈 수 있다. 모두 무해한 방향(눌림 고착 없음)이지만 "청소 중 입력 동결" 기대와 어긋난다.
- **발생 조건:** `iReleaseDeferMs > 0` + 키를 누른 채(보류 발생) 청소 모드 진입, 또는 청소 중 보류 타이머 만료. 틈새는 지연값(수십 ms) 수준이라 조건이 좁다.
- **영향:** 유령 뗌 1회 또는 종료 직후 눌림 1회의 `Flush` 재주입. 데이터 손상 없음.
- **근거:** 청소 분기가 `pending_up` 검사보다 먼저 반환됨(`296-304` vs `327` 이하). `drain`은 `cleaning_mode`를 확인하지 않음(`380-406`). 마우스 청소는 의도적 통과(주석 명시)라 반증되나, **키보드 보류 뗌은 주석·테스트 모두 없음**을 확인했다.
- **반증 확인:** `Defer`는 홀드 중인 키에서만 발생하고 청소 진입은 버튼·핫키·Enter(눌림 차단 대상)라 진입 순간 손가락이 키 위에 있을 확률이 낮다. 또한 방출돼도 `INJECT_MARK`라 필터가 재처리하지 않는다. 그래서 Low로 분류한다.
- **호출/영향 범위:** caller `hook_proc`(청소 분기) · 타이머 `WM_TIMER/WM_KCFIX_INJECT → drain`; 영향은 `pending_up`·`inject` 큐·`SendInput` 1회.
- **권장 수정 방향:** `set_cleaning(true)` 시 `pending_up`을 전부 확정 방출(즉시 `drain(true)` 상당 처리)하거나, 반대로 `drain`의 타이머 경로에서 청소 중이면 만료분을 청소 종료까지 보류. 둘 중 하나로 일관.
- **필요한 회귀 테스트:** (Unit) `release_defer` + 청소 진입 시나리오: `Down→Up(Defer)→cleaning=true→due_releases` 후 보류 잔량이 정책대로(0 또는 유지)임을 assert. 현재 `release_defer_*` 테스트 4건에 청소 조합이 없음을 확인.

### [ISSUE-003] `bMinimizeOnLaunch`만 불리언 해석 규칙이 다르다

- **위치:** `src/config/storage.rs` — `115-120` (`bMinimizeOnLaunch`는 `v == "1"`, 같은 파일의 다른 7개 플래그는 `v != "0"`). 규칙 테스트 `load_treats_any_nonzero_boolean_as_true` (`515-531`)
- **우선순위:** Low
- **신뢰도:** Confirmed
- **문제:** "0이 아니면 참" 규칙이 테스트로 강제돼 있는데(`bPlaySound=2`·`yes` → 참) 유독 시작 최소화만 `"1"`이 아니면 거짓이다. 손으로 파일을 고친 사용자가 `bMinimizeOnLaunch=yes`라 쓰면 다른 플래그와 반대로(꺼짐) 동작한다.
- **발생 조건:** 설정 파일을 직접 편집하고 비정형 참값(`yes`, `2` 등)을 쓸 때. UI 저장값은 항상 0/1이라 UI 경로에서는 발생하지 않는다.
- **영향:** 시작 시 최소화 여부 1회성 어긋남. 보호 기능·데이터 무결성과 무관.
- **근거:** 코드 직접 확인. 테스트가 `bMinimizeWhenLaunch` 별칭까지 묶어 규칙을 명시하므로 의도된 분기가 아님을 확정했다.
- **반증 확인:** UI 경로·기본값·별칭 처리에는 영향이 없어 피해가 수동 편집으로 한정됨을 확인했다. 그럼에도 테스트가 문서화한 규칙 위반이므로 수정 대상이다.
- **호출/영향 범위:** `parse ← load/try_load ← bootstrap·on_save·persist_with`. 영향 모듈 없음(표시 동작만).
- **권장 수정 방향:** `s.minimize_on_launch = v != "0";`로 통일하고, 테스트에 `bMinimizeOnLaunch=yes → true` 케이스 1줄 추가.
- **필요한 회귀 테스트:** 위 케이스. 입력 `bMinimizeOnLaunch=yes` → 기대 `minimize_on_launch == true`.

### [ISSUE-004] 시작 시 스테이징 청소가 진행 중인 업데이트 교체 파일을 지울 수 있다

- **위치:** `src/updater/staging.rs` — `cleanup_stale_staging` (`333-345`), `is_staging_file_name` (`321-326`); 호출점 `src/app/bootstrap.rs:111`
- **우선순위:** Low
- **신뢰도:** Likely
- **문제:** 청소는 이름 패턴으로 %TEMP%의 `kcfix-updater_helper_*`·`keyboardchatteringfix_update_*`를 전부 삭제한다. 실행 중인 헬퍼 본인은 이미지 잠금으로 건너뛰지만, **아직 실행되지 않은 교체 파일(잠금 없음)은 삭제된다**. 헬퍼가 구 프로세스 종료를 최대 120초 기다리는 동안 사용자가 앱을 재실행하면, 새 프로세스의 시작 청소가 그 교체 파일을 지우고 헬퍼의 교체 직전 해시 검사가 실패해 업데이트가 깨진다(헬퍼는 롤백·이전 버전 재실행 후 오류 대화상자 — 데이터 손상은 없음).
- **발생 조건:** 업데이트 헬퍼 대기 중(수 초~2분, 보통 수 초) 수동 재실행. 단일 인스턴스 가드와 무관하게(구 프로세스는 종료 중) 새 프로세스가 뜰 수 있다.
- **영향:** 업데이트 실패 1회(재시도로 복구). 필터 공백 가능성은 헬퍼의 이전 버전 재실행(`should_relaunch_previous`)으로 완화된다.
- **근거:** 청소 주석이 "아직 실행 중인 헬퍼는 잠금으로 건너뛴다"고만 보장하고 교체 파일은 언급하지 않음(`328-332`). `prepare_update` 실패 시 삭제(`239-243`)와 달리 진행 중 보호가 없다.
- **반증 확인:** 단일 비행 플래그(`UPDATE_IN_PROGRESS`)는 프로세스 내 메모리라 재실행된 프로세스에는 소용없음을 확인했다. 단일 인스턴스 뮤텍스는 종료 중인 구 프로세스가 잠시 쥐고 있어(`--after-pid` 대기) 재실행이 바로 성공하지 않을 수 있으나, 10초를 넘기면(느린 종료) 그대로 진입하므로 방벽이 되지 못한다.
- **호출/영향 범위:** caller `bootstrap::run`(별도 스레드); callee `is_staging_file_name`. 영향 모듈: 헬퍼 교체 흐름(`crates/kcfix-updater`), `launch_helper` 이후 단계.
- **권장 수정 방향:** 교체 파일도 잠금(쓰기 핸들 유지)으로 보호하거나, 청소를 "N시간 이상 경과" 파일로 한정. 또는 헬퍼 대기 시작 시점에 마커 파일을 두고 청소가 마커가 있는 동안은 건너뛰게 한다.
- **필요한 회귀 테스트:** (Integration) 가짜 스테이징 파일 + 진행 중 마커(또는 잠금) 상황에서 `cleanup_stale_staging`이 새 파일은 지우고 진행 중 파일은 남김을 assert. 현재 `staging_leftovers_are_recognized_by_name_only`는 이름 판별만 커버.

---

## 5. Potential Functional Gaps

버그가 아니라 "현재 목적상 빠졌을 가능성이 있는" 안정성 보완점이다. 추정은 버그처럼 표현하지 않는다.

- **관측 로그가 GUI에서 버려짐 (Likely Gap).** `bootstrap::run`이 `tracing_subscriber::fmt`을 초기화하지만(`src/app/bootstrap.rs:79-84`), GUI 빌드는 `windows_subsystem = "windows"`라 콘솔이 없어 느린 틱 경고(`stats.rs:196-222`)·훅 실패·업데이트 경로의 `warn/error!`가 실제로는 어디에도 기록되지 않는다. 과거 "기동 직후 응답없음" 같은 이슈가 재발하면 원인 추적 수단이 없다. 방향: `%TEMP%` 또는 설정 폴더의 회전 로그 파일 1개(크기 상한 포함)로 연결. 단, 훅 핫패스에서의 로깅은 금지(현 규칙 유지).
- **매니페스트 최소 버전·폐기(revocation) 없음 (Likely Gap).** 서명·만료·핀고정·이중 해시는 탄탄하나, 서명 키가 탈취된 경우 이미 서명된 구 매니페스트의 재전송(다운그레이드 유도)을 막을 장치가 만료 시각(선택 필드, 구 매니페스트엔 없음)뿐이다. 방향: 바이너리에 최소 허용 버전 상수 또는 키 회전 계획(`docs/UPDATE.md`에 명문화).
- **프록시·오프라인 환경 안내 부족 (추정).** `ureq` 에이전트에 프록시 설정이 없고, 기업망에서 업데이트 검사가 실패하면 기술 상세만 표시된다. Scoop/winget 설치 감지 후 관리자 안내(`update.rs:86-97`)는 잘 돼 있다. 방향: 시스템 프록시 준수 또는 실패 메시지에 수동 다운로드 경로 안내.
- **키별 통계 개별 초기화 없음 (추정).** 전체 초기화(`reset_stats`)만 있고 특정 키의 히스토그램·카운터만 지우는 경로는 없다. 스위치 교체 후 해당 키의 권장값이 옛 데이터에 오염된다. 방향: 키·마우스 탭 행 컨텍스트에 "이 키 통계 지우기"(작은 enhancement, 버그 아님).
- **조사했으나 Gap으로 채택하지 않은 것:** (a) 일시 중지·보호 꺼짐·주입 제외 상태에서 훅이 상태 전이를 생략하는 것 — 재개 후 fail-open(통과 방향)이라 안전하고 의도된 설계다. (b) `InjectQueue` 32칸 초과분 침묵 삭제 — 실제 손가락으로 32개 동시 보류는 불가능해 dead-code에 가까운 방어 한계다. (c) 휴대 모드 판정이 실행 중 뒤집히는 경우 — ini 삭제라는 인위적 조작이 필요하다. (d) CSV 수식 인젝션 — 셀 값이 내부 VK 이름·시각·숫자뿐이라 불가하다.
- **보안 특기:** 업데이트(서명+핀+크기+이중 해시), 자동실행 인용 경로, 작업 XML 이스케이프, `schtasks` 무창 실행, 셸 실행의 워커 스레드 분리까지 점검했고, password·토큰·개인정보 저장, SQL·셸 인젝션, 경로 탈출(가져오기는 사용자 선택 경로가 의도)에 해당하는 문제를 찾지 못했다.

## 6. Documentation Mismatches

- **`AGENTS.md` 없음.** 지시문이 확인을 요구한 문서이나 저장소 루트·추적 파일(`git ls-files`) 어디에도 없다. README·CLAUDE·GEMINI가 역할을 대신하고 있어 실무 영향은 없다. (없음을 명시)
- **CLAUDE.md "설정 쓰기는 `persist_live`와 `on_save`뿐" — 현재는 3곳.** 가져오기(`src/ui/files.rs:141`)가 `config::save`를 직접 호출한 뒤 `on_save`가 한 번 더 저장한다. 의도(가져온 값 + 이 PC 통계로 디스크 선반영)는 타당하고 무해한 중복이나, "둘 다 디스크 값을 다시 읽어 병합한다"는 서술이 가져오기 경로를 설명하지 않는다. `grep`으로 `config::save` 호출점 3곳을 확정했다.
- **GEMINI.md 성능 계약 1항이 구버전.** "유일한 Win32 호출은 차단 시 사운드"라고 돼 있으나, 현재 훅 경로는 떼기 확인용 `SetTimer`·`PostThreadMessageW`도 허용한다(훅 스레드 타이머 1개). CLAUDE.md는 이미 갱신돼 있고 GEMINI.md만 뒤처졌다.
- **위 3건 외 불일치 없음.** README의 설정 샘플 20개 필드·`key.XX`·휴대 모드·아키텍처 그림·버전 동기화 서술은 코드·테스트(`version_sync` 6건)와 일치함을 확인했다.

## 7. Recommended Fix Plan

실제 코드는 수정하지 않는다. 아래는 순서 제안뿐이다.

### Phase 1 — Immediate

데이터 손상·주요 기능 실패에 해당하는 확정 항목이 없으므로, Phase 1은 ISSUE-001의 저비용 완화로 한정한다.

- ISSUE-001: `restart()`의 구 스레드 짧은 join(수백 ms) + 실패 시 포기, `send_probe`의 `SendInput` 반환값 확인. 통계 오염·이중 사운드의 발생 창을 닫는다.
- ISSUE-003: 1줄 통일(`!= "0"`) + 테스트 1줄. 즉시 가능.

### Phase 2 — Stability

- ISSUE-002: 청소 진입 시 보류 뗌 정책 확정(즉시 방출 vs 종료까지 보류) + 단위 테스트.
- ISSUE-004: 스테이징 진행 중 보호(잠금 유지 또는 경과 시간 조건) + 통합 테스트.
- 관측 로그 파일 회전 도입(5장의 Likely Gap 1). 훅 핫패스 무로깅 규칙은 유지.
- 가져오기 저장 경로의 문서화(CLAUDE.md 1줄) 또는 `config::save` 직접 호출을 `persist_with` 계열로 일원화.

### Phase 3 — Structural

- 최소 허용 버전 상수·키 회전 계획 문서화(5장의 Likely Gap 2).
- E2E 스크립트(`hook_session_e2e`·`features_e2e`·`update_swap_e2e`)의 정기 실행 기록. 이번 감사에서 미실행이므로 다음 릴리스 전 수동 실행을 권장.
- 키별 통계 개별 초기화 같은 소규모 UX 보완은 별도 enhancement로 분리(버그 아님).

## 8. Test Recommendations

"테스트 추가"가 아니라 입력 조건과 기대 결과를 명시한다. 기존 스위트(188건)는 유지하고 아래를 더한다.

- **Unit — 불리언 규칙 통일(ISSUE-003):** 입력 `[KeyboardChatteringFix]\nbMinimizeOnLaunch=yes` → 기대 `minimize_on_launch == true`. 기존 `load_treats_any_nonzero_boolean_as_true`에 1줄.
- **Unit — 청소×떼기 확인(ISSUE-002):** `set_release_defer_ms(30)` 후 `Down(0)→Up(Defer)→cleaning_mode=true→due_releases` 입력 → 기대는 채택 정책대로 보류 잔량 0(즉시 방출) 또는 1(종료까지 보류) 중 하나를 assert. 정책 결정이 선행돼야 함.
- **Unit — 세대 가드(ISSUE-001):** `drain`에 세대 인자 도입 시, 구 세대 `drain(true)` 호출 → 기대 방출 0건·`pending_up` 유지.
- **Integration — 재시작 인계(ISSUE-001, 데스크톱 환경 한정):** `spawn_hook_thread → restart → 구 스레드 join(타임아웃 5초)` → 기대 `HOOK_THREAD_ID`가 새 스레드 1개만 가리키고 구 스레드 종료. 기존 `hook_runs_on_a_dedicated_thread_and_stops` 옆에 배치.
- **Integration — 스테이징 보호(ISSUE-004):** 임시 디렉터리에 가짜 `keyboardchatteringfix_update_*.exe`(최근) + 경과 파일(오래됨) 배치 후 `cleanup_stale_staging` → 기대 오래된 것만 삭제(또는 마커 도입 후: 마커 존재 시 전부 유지). `TEMP` 격리(`with_isolated_appdata` 패턴) 필수.
- **Concurrency — 중복 재시작:** 두 스레드에서 `restart()` 동시 호출 → 기대 훅 스레드 최대 1개 생존, 패닉 없음. `try_begin_update_parallel_exactly_one_winner` 패턴을 따른다.
- **E2E(수동, 릴리스 전):** `hook_session_e2e`(훅·UI 정지·세션 종료 저장), `features_e2e`(떼기 확인·마우스·단축키·자동 일시 중지), `update_swap_e2e`(헬퍼 연속 교체). 이번 감사 미실행 — `scripts/e2e/README.md` 절차대로 다음 릴리스 전 실행.
- **Regression — 기존 약한 assertion 강화:** 프로젝트 철칙(`<= 11 → == 10`류)에 따라, 이번 수정과 만나는 기존 테스트의 범위 assertion을 정확한 값으로 좁힌다. 새 hollow 테스트는 금지.
- **Platform-specific:** 본 프로젝트는 Windows 전용이므로 Linux/macOS 이식 테스트는 범위 밖. 대신 DPI(125%/150%)·명암 테마·승격/비승격 조합의 수동 체크리스트를 E2E에 유지한다.

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Acceptable | 필터 코어·설정·업데이트 검증이 정확하고 188건 테스트가 보증. 남은 것은 좁은 조건의 Low 3건 + Likely Medium 1건 |
| Runtime Stability | Acceptable | 훅 전용 스레드·재진입 가드·diff 갱신·락 순서(SAVED→SHARED 단일, 역전 없음) 확인. 워치독 재시작 틈새만 잔존 |
| Data Integrity | Good | 원자 저장+`.bak`+백업 폴백, 이중 해시, 통계 병합 저장. 유실은 크래시 시 최대 5분 통계뿐(설계) |
| Error Resilience | Good | 저장 실패·핫키 충돌·헬퍼 부재·프로브 실패 경로마다 사용자 알림 또는 폴백. `send_probe`의 `SendInput` 반환 무시 같은 작은 구멍만 잔존 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도이며 README·매니페스트와 일치. 이식성 결함이 아니라 범위 선언으로 평가 |
| Test Confidence | Good | 188/188 통과, fmt·clippy 깨끗, 경계·폴백·동시성 단위 테스트 보유. E2E만 수동(미실행)으로 남음 |

**실제로 먼저 수정할 문제 3개**

1. **ISSUE-001 (워치독 재시작 인계)** — Medium/Likely. 통계 무결성과 직결된 유일한 중간 위험. join-짧게 + `SendInput` 반환 확인.
2. **ISSUE-003 (`bMinimizeOnLaunch` 규칙 통일)** — Low/Confirmed. 1줄 수정+테스트 1줄, 즉시 처리 가능.
3. **관측 로그 파일화 (5장 Gap 1)** — 다음 "원인 불명" 이슈 발생 전에 추적 수단을 확보. 그 전에는 ISSUE-001류 타이밍 문제를 재현·확정할 방법이 없다.

---

## 10. 적용 기록 (2026-10-18)

본 감사 직후 사용자 요청으로 위 제안을 전부 구현했다. E2E 수동 항목(8장의 릴리스 전 절차)은 실기 GUI가 필요해 미실행으로 남는다.

| 제안 | 적용 내용 | 테스트 |
|---|---|---|
| ISSUE-001 | `HOOK_GEN` 세대(생성 시 획득·`abandon` 시 은퇴), 구 스레드 종료 방출 건너뛰기, `drain`의 타이머 재개설(인계), `start`의 인계 킥, `send_probe` 전송 실패 시 재시작 대신 다음 주기 재시도 | `restart_hands_over_pending_releases_to_one_thread`, `concurrent_restarts_leave_one_hook_thread` |
| ISSUE-002 | 청소 진입(`set_cleaning(true)`) 시 `flush_deferred_releases()`로 보류 뗌 즉시 방출(인젝터 경유) | `flush_deferred_releases_empties_pending` |
| ISSUE-003 | `v == "1"` → `v != "0"` 1줄 통일 | `load_treats_any_nonzero_boolean_as_true`에 `yes` 케이스(수정 전 실패 확인) |
| ISSUE-004 | `cleanup_stale_staging`을 1시간 경과 파일만 삭제하도록 변경(`cleanup_staging_dir` 분리) | `cleanup_keeps_fresh_staging_and_drops_only_old` |
| 관측 공백 Gap 1 | `src/ui/diag.rs` 회전 파일 로그(`%TEMP%\kcfix-diag.log`, 256KB·1세대), 느린 틱·훅 재시작 연결 | `lines_accumulate_in_order`, `oversize_log_rotates_once` |
| 키별 초기화 Gap | `Debouncer::clear_key_stats`(전체 합계 차감·히스토그램·해당 로그행 정리, 기준값 유지) + 키 탭 "통계 지우기" 버튼(ID 711·한/영 문구·저장 연동) | `clear_key_stats_forgets_one_key_only`, layout 겹침·카드·문구 테스트 통과 |
| 문서 불일치 | CLAUDE(쓰기 경로 예외·`diag` 모듈·진단 로그 안내), GEMINI(훅 허용 호출 갱신), UPDATE(구 매니페스트 재전송 절 신설) 수정 | 문서이므로 테스트 없음 |
| 프록시·개별 초기화 외 추정 Gap | 프록시는 미적용(향후 과제 유지), 키별 초기화는 위 버튼으로 해소 | — |

*감사 종료. 본 절은 감사 이후의 적용 기록이며, 감사 본문(1~9장)의 판단은 작성 시점 기준이다.*

---

## 11. 성능 핫패스 감사 (2026-10-19)

- 범위: 입력 지연·CPU·메모리·락 경합에 직접 닿는 경로 — 훅 콜백(`hook_proc`/`mouse_proc`), 500ms 타이머 틱, 페인트, 떼기 확인 타이머, 시작 시점, 업데이트 해시.
- 방법: 1~10장과 동일(문서 → CodeGraph 4회 → 소스 직접 열람 → 테스트 실행). **코드는 수정하지 않았다.** 프로파일러·실기 GUI 측정은 하지 않았으므로, 아래 수치는 정적 근거 기반 추정치이며 측정값처럼 표현하지 않는다.
- 전체 결론: **핫패스는 잘 설계돼 있다.** 이벤트당 비용은 락 1회 + O(1) 판정 수준이고, UI 쪽 락 구간은 memcpy 규모, Win32 호출은 락 밖(훅의 비동기 사운드 제외)으로 분리돼 있다. Critical/High 없음, 확정 이슈 2건(Low).

### 11.1 경로별 예산 분석

| 경로 | 주기 | 락·할당 | 판단 |
|---|---|---|---|
| `hook_proc` 키 이벤트 | 입력 시 | `SHARED` 1회, `decide_event` O(1), QPC 1회, 할당 없음 | 정상. `VK_PACKET`·주입·표식은 락 전에 통과 |
| `mouse_proc` 이동·휠 | 초당 수백 회 | 락을 잡기 전 분류·반환, 버튼만 락 | 정상. 이동 폭주가 락을 건드리지 않음 |
| 차단 시 사운드 | 차단 시 | 락을 쥔 채 `PlaySoundW`(비동기)·폭은 수십 µs | 정상(설계). 아래 반증 참조 |
| 500ms 틱(`update_stats`) | 2Hz | 스냅샷 1회(수십 KB 복사), 렌더 순수, diff 후 변경분만 `SetWindowTextW` | 정상. 재진입 가드 + 250ms 초과 기록 |
| 전경 틱 | 2Hz | 작은 락 3회 + 문자열 복제, 프로세스 조회는 창 변경 때만, `SHARED`는 변경 때만 | 정상 |
| 기록·키 탭 리프레시 | 2Hz | 탭 숨김·키 불변 시 조기 반환, 변경 때만 스냅샷·렌더 | 정상. 상한 5000행, 가상 리스트라 표시는 가시행만 |
| 떼기 확인 타이머 | 보류 중에만 10ms 주기 | 기상당 락 1회 + 512칸 순회(수 µs) | 정상. 아래 수치 참조 |
| 페인트 | 무효화 때만 | 더블버퍼 `BitBlt`, 브러시 캐시, 펜은 생성·삭제 쌍 | 정상 |
| 시작 1회 | 1회 | 사운드 `is_file`·레지스트리·스테이징 청사는 1회성(청소는 별도 스레드) | 정상 |

### 11.2 이슈

### [PERF-001] 업데이트 해시가 파일 전체를 RAM에 적재한다

- **위치:** `src/updater/http.rs` — `download_and_verify` (`38-56`, `Vec`에 `size+1`까지 적재); `crates/kcfix-updater/src/lib.rs` — `sha256_file` (`113-118`, 무제한 `fs::read`)
- **우선순위:** Low
- **신뢰도:** Confirmed
- **문제:** 아티팩트 전체를 메모리에서 해시한다. 앱 측은 매니페스트 선언 크기(상한 512MB+1)로 묶이지만, 헬퍼 측은 파일 크기 그대로 읽는다. 한 번의 업데이트에서 원본이 메모리에 두 번(다운로드 버퍼, 재검증 버퍼) 올라간다.
- **발생 조건:** 항상. 다만 실제 아티팩트는 수 MB라 일시적 증가분도 수십 MB 수준이다.
- **영향:** 저사양 PC에서 순간 메모리 스파이크. 데이터 손상·지연 없음(업데이트는 워커·헬퍼에서 수행).
- **근거:** 코드 직접 확인. 상한·이중 검증 자체는 정확하므로 보안 이슈가 아님을 확인했다.
- **반증 확인:** `take(size+1)` + `SizeOutOfRange` 검사가 악성 대용량을 막고(`45-47`), 실제 릴리스 산출물은 한 자릿수 MB라 현실 피해가 없다. 그래서 Low이며, 수정보다 향후 과제(스트리밍 해시)가 맞다.
- **호출/영향 범위:** `prepare_update → download_and_verify`, 헬퍼 `update_executable_with` 2단계. 필터·UI 경로와 무관.
- **권장 수정 방향:** `Read` 스트리밍 해시(청크 64KB)로 교체. 동작·테스트 기대값은 그대로.
- **필요한 회귀 테스트:** 기존 해시 테스트 + 100MB급 임시 파일 해시 일치 테스트(스트리밍/일괄 동일 결과).

### [PERF-002] 지속 부하 테스트의 상한이 변별력이 없다

- **위치:** `src/ui/stats.rs` — `render_and_diff_sustained_tick_load` (`539-553`)
- **우선순위:** Low
- **신뢰도:** Confirmed
- **문제:** 1만 회 render+diff에 10초를 허용한다(회당 1ms). 현재 구현은 그보다 1~2자릿수 빠를 것으로 추정되나, 렌더가 느려져도(예: 렌더에 I/O·락이 섞여도) 10초 안에는 통과해 회귀 가드로 작동하지 않는다.
- **근거:** 테스트 코드 직접 확인. 프로젝트 철칙("약한 assertion 강화")에도 어긋난다.
- **권장 수정 방향:** 상한을 1초로 강화(여전히 넉넉). 실패하면 그때 원인을 본다.
- **필요한 회귀 테스트:** 상한 강화 자체가 테스트 변경이다. 별도 신규 테스트 불필요.

### 11.3 반증되어 이슈에서 제외한 것

- **차단 시 `PlaySoundW`가 `SHARED` 락 안에서 실행됨** (`hook.rs` `apply` → `drop_with_sound(&g)`): `SND_ASYNC`라 반환이 빠르고 파일 I/O·할당이 없다(사전 인코딩 버퍼). 빠른 연속 차단에도 입력 지연으로 번지지 않으므로 정상.
- **UI 쪽 `SHARED` 점유**: `on_save`(256 순회+복사)·`persist_*`(카운터 복사)·`csv_snapshot`(최대 5000행 복제, 사용자 동작 때만) 모두 memcpy 규모이며, `set_autostart`·`config::save`·`hotkeys::apply`·`ShellExecuteW`는 락 밖이다. 훅 대기 시간에 미치는 영향은 µs 단위.
- **기록 탭 5000행 재렌더**: 변경된 틱·보이는 탭에서만, 행 문자열은 캐시 후 가상 리스트가 가시행만 조회. 상한이 있어 발산하지 않는다.
- **히스토그램(256×41, 약 43KB) 스냅샷**: 키 탭이 보일 때 2Hz 복사로 수십 µs 수준. `take_snapshot`이 버퍼를 락 밖에 미리 할당해 락 안에서는 복사만 한다(`keys_page.rs:210-223`).
- **`top_chattered_key` 256 순회**: 스냅샷 락 안에서 실행되나 정수 비교 256회라 무시 가능.
- **마우스 훅 상시 비용**: 필터가 꺼져 있으면 훅 자체를 설치하지 않아(`set_mouse_filter`), 이동 이벤트 비용은 0이다.
- **`timeBeginPeriod` 미사용**: `Instant`(QPC) 측정이라 타이머 해상도를 올릴 이유가 없고(부트스트랩 주석), 전력·전역 부작용이 없다.

### 11.4 권장 (코드 수정 없음, 순서만)

1. PERF-002 상한 강화(1초) — 즉시 가능, 위험 없음.
2. PERF-001 스트리밍 해시 — 다음 업데이트 만질 때 함께. 메모리 상한 테스트를 동반.
3. 측정 기반 확인(향후): 실기에서 500ms 틱 분포(`kcfix-diag.log`에 이미 느린 틱이 기록됨)와 `render_and_diff` 실측값을 모아 상한을 실측으로 교체.

### 11.5 테스트 증거

- `cargo test --workspace` (exit 0): 195건 전부 통과(3+2+10+169+5+6+0). 성능 관련 포함: `render_and_diff_sustained_tick_load`, `render_*`/`diff_*` 전건, `history` 렌더 테스트, `keys_page` 행 테스트, `debounce` 33건, `hook` 7건, `diag` 2건.
- `cargo fmt --check`·`clippy -D warnings`: 깨끗(본 감사에서 코드 변경 없음).
- 측정하지 못한 것: 실기 입력 지연(QPC 수준이라 프로파일러 필요), GUI 틱 실측 분포, 5000행 만재 시 렌더 시간. 런타임 재현·프로파일링은 하지 않았으므로 타이밍 이슈는 `Likely` 이하로만 분류했다(해당 없음).

*성능 감사 종료. 코드는 수정하지 않았으며, 본 장은 기존 문서(1~10장)에 추가된 것이다.*

### 11.6 적용 기록 (성능 2건)

사용자 요청으로 11.2의 2건을 구현했다(테스트 포함, 게이트 통과).

- PERF-002: 상한 10초 → 1초로 강화. 통과(0.02초대, 여유 큼).
- PERF-001: `download_and_verify`를 64KB 청크 스트리밍(해시+파일 동시 기록, 실패 시 부분 파일 삭제)으로 교체하고, 미사용이 된 `sha256_hex`와 재수출을 제거(프로젝트 규칙). 헬퍼 `sha256_file`도 스트리밍으로 교체. 테스트: `stream_hashed_writes_and_hashes_incrementally`(앱, 확정 empty 다이제스트 오라클), `streamed_file_hash_matches_oneshot_hash`(헬퍼, 기존 모듈에 병합). 전체 197건 통과.

---

## 12. 보안 심층 감사 (2026-10-19)

- 범위: 기능과 연관된 보안 위험 — 업데이트 신뢰 체인, 윈도우 메시지, 권한·자동실행, 설정·임시 파일, 입력 훅의 민감 데이터, 버전 파싱. 스타일·취향 제외.
- 방법: 1~11장과 동일(문서 → CodeGraph 4회 → 소스 직접 열람 → 테스트 실행). **감사 단계에서는 코드를 수정하지 않았다**(11.6의 성능 구현은 별도 사용자 요청). 런타임 침투·실기 UAC·네트워크 MITM은 수행하지 않았다.
- 전체 결론: **심각한 문제 없음.** 업데이트 신뢰 체인(서명·핀고정·크기·만료·이중 해시)은 전송계층과 독립적으로 위조를 막는다. 남은 것은 로컬 DoS 수준 1건(Low) + 관찰 1건이다.

### 12.1 위협 모델(전제)

- 공격자는 같은 세션의 저권한 프로세스 실행이 가능하다고 가정한다(예: 샌드박스 렌더러).
- 같은 사용자·중간 무결성 공격자는 이미 파일 삭제·프로세스 종료가 가능하므로, 그 수준에서만 통하는 문제는 심각도를 올리지 않는다.
- 네트워크 공격자는 서명 키 없이 위조가 불가능해야 한다(전송계층과 무관하게 성립해야 함).

### 12.2 이슈

### [SEC-001] 인증 없는 윈도우 메시지로 종료·상태 변경이 가능하다

- **위치:** `src/ui/ids.rs` — `WM_UPDATE_EXIT` (`138`, `WM_APP + 2`, 인증자 없음); `src/ui/window.rs` — 분기 (`603-608`, `335-336` 경유 메뉴 명령 포함); `src/ui/commands.rs` — `MENU_EXIT` (`177-179`, 확인 없음)
- **우선순위:** Low
- **신뢰도:** Likely
- **문제:** 창 클래스명(`KeyboardChatteringFixMain`)은 고정·공개돼 있어 같은 세션의 프로세스는 창을 찾아 `WM_UPDATE_EXIT`를 게시하면 즉시 종료(`DestroyWindow`, 카운터는 `WM_DESTROY`에서 저장됨), `WM_COMMAND`에 메뉴 ID(1001~1017)를 담으면 종료·통계 초기화·청소 모드·업데이트 확인 등을 실행할 수 있다. UIPI 기본값 덕에 `WM_APP+` 메시지는 저무결성→중간무결성에 차단되지만, `WM_COMMAND`(0x0111)는 허용 목록이라 저무결성 프로세스도 메뉴 명령을 발사할 수 있다. 가장 영향이 큰 것은 확인 없는 즉시 종료와 청소 모드 진입(키보드 입력 동결, Enter·버튼·5분 타임아웃으로 복구)이다.
- **발생 조건:** 같은 세션에서 코드 실행 + 창 핸들 발견. 원격·네트워크 불가.
- **영향:** 필터 중단(재실행으로 복구), 통계 초기화(되돌릴 수 없음, 확인 대화상자는 UI 경로에만 있음). 데이터 유출·권한 상승 없음.
- **근거:** 코드 직접 확인. `ChangeWindowMessageFilterEx` 호출이 저장소 어디에도 없어 기본 UIPI가 그대로 적용됨을 확인했다.
- **반증 확인:** (1) 같은 사용자·중간 무결성 공격자는 `taskkill`·설정 삭제로 동일 효과를 내므로 새로운 원자적 위협이 아니고, (2) 종료 시 `WM_DESTROY`가 카운터를 저장해 설정·통계 손실이 없으며, (3) 서명·자동실행·파일 교체 등 상위 권한 동작은 메시지로 건드릴 수 없다. 그래서 Low로 분류한다.
- **호출/영향 범위:** 외부 프로세스 → `wndproc` → `commands::run` 전 메뉴·`DestroyWindow`. 필터 스레드·업데이트 체인과 무관.
- **권장 수정 방향:** `WM_UPDATE_EXIT`에 기동 시 생성한 64비트 난수(nonce)를 `wParam`/`lParam`으로 동봉·검증(저비용, 동일 사용자 장난까지 차단). 메뉴 경로(`WM_COMMAND`)는 시스템이 생성하므로 nonce를 붙일 수 없어 잔류 수용하되, `MENU_EXIT`에 확인 대화상자를 다는 것을 검토(단, 업데이트 후 자동 종료 경로와 충돌하지 않게 내부 플래그로 구분).
- **필요한 회귀 테스트:** nonce 불일치 `WM_UPDATE_EXIT`는 무시·일치는 종료를 assert하는 `session_reply`류 순수 판정 테스트(현재 세션 종료 판정 테스트 옆에 배치). GUI 발송 테스트는 금지(프로젝트 철칙).

### [SEC-002] 단일 인스턴스 뮤텍스 선점으로 자동실행이 조용히 꺼질 수 있다(관찰)

- **위치:** `src/app/instance.rs` — `try_acquire_named` (`33-58`), `Local\KeyboardChatteringFix_SingleInstance`
- **우선순위:** Low
- **신뢰도:** Speculative
- **문제:** 같은 세션의 프로세스가 뮤텍스 이름만 선점하면, 부팅 시 `--autostart` 경로는 대화상자 없이 조용히 종료돼 필터가 꺼진 채로 남는다. 일반 경로는 "이미 실행 중" 대화상자가 떠서 드러난다.
- **근거:** 코드 직접 확인. `CreateMutexW` 실패 시 fail-open(`54-55`)은 별개로, 리소스 고갈 상황에서만 탄다.
- **반증 확인:** 선점 자체가 같은 세션 코드 실행을 요구하고(그 수준이면 Run 키 삭제 등 더 직접적인 방법이 있음), 지속 상주하려면 부팅마다 선점해야 해서 현실성이 낮다. 그래서 이슈가 아닌 관찰로 기록한다.
- **권장 수정 방향:** 없음(수용). 원하면 `--autostart` 조용 종료 전에 윈도우 클래스 존재 여부로 실제 인스턴스를 확인하는 1줄 보강을 검토.

### 12.3 반증되어 이슈에서 제외한 것(업데이트 신뢰 체인 중심)

- **매니페스트 위조**: 64KB 상한 → JSON 파싱 → 서명·키·길이 검증 → 정식(canonical) 바이트 검증(`manifest.rs:30-66`) 순서로, 서명 전 필드를 신뢰하지 않는다. 서명 키 없이는 위조 불가이므로 TLS MITM은 가용성 문제로만 축소된다(전송계층 독립).
- **아티팩트 바꿔치기**: 핀고정 URL이 태그에서 도출된 정확한 URL과 일치해야 하고(`is_valid_artifact_url`), 크기·SHA-256을 다운로드 시와 헬퍼 교체 직전에 각각 검증한다. URL에 태그 외 입력이 들어갈 여지가 없다(`tag = v + 숫자.점 버전` 강제).
- **버전 파싱**: `version_tuple`은 숫자·점만 허용하고 오버플로우는 `unwrap_or(0)`으로 fail-closed(패닉 없음). 빈 조각·비어 있음은 거부한다.
- **정식 JSON 차이(DEL 등)**: Rust `is_control`과 Python `json.dumps`의 이스케이프 범위가 달라 제어문자가 들어간 매니페스트는 서명 불일치로 실패한다 — fail-closed이며, 서명 스크립트의 고정 필드에 제어문자가 들어갈 수 없어 가용성에도 영향이 없다.
- **다운그레이드 재전송**: 11장 이전부터 알려진 잔류(선택적 `expires_at` + 버전 비교). 키 탈취를 전제로 하며 완화책은 `docs/UPDATE.md`에 문서화했다. 중복 기록하지 않는다.
- **헬퍼 CLI**: 셸 없이 `Command` 직접 실행, 경로는 앱이 지정한 값만. `--relaunch-arg=` 형태는 구 헬퍼와의 계약 테스트가 보증. 환경 변수(`KCFIX_LANG`·준비 파일)는 표시·동기화 용도이며, 준비 파일 쓰기 내용은 pid 숫자뿐이다.
- **자동실행·작업**: Run 값은 인용 경로 + 고정 플래그, 자기 값만 변경. 작업 XML은 이스케이프 처리. `schtasks`는 무창·고정 인자.
- **설정 파일 변조**: 모든 수치 클램프, 불리언 규칙 통일(10장 적용), 미지정 키 무시, 핫키·앱 목록은 파싱 실패 시 안전 기본. 설정으로 코드 실행·권한 변경에 이를 수 없다.
- **가져오기 검증이 약함**(헤더 1줄 확인): 값 자체가 전부 클램프·검증되므로 통과돼도 해가 없다. 확인했다.
- **CSV 수식 인젝션**: 셀 어휘가 내부 VK 이름·시각·숫자로 고정돼 불가. 확인했다.
- **사운드 경로**: 존재하는 파일만, 재생만(`PlaySoundW`), 실패 시 시스템음 폴백. 실행 아님.
- **프로브·주입 표식 위조**: 표식은 훅 안에서만 의미를 갖고, 프로브 카운트는 훅이 살았을 때만 증가하므로 죽은 훅을 속일 수 없다. 표식을 단 외부 입력이 필터를 우회해도 공격자 자신의 입력일 뿐이다.
- **민감 데이터**: 후크가 평문 키 입력을 보는 것은 설계상 필수이며, 키별 내용은 메모리에만(로그 상한 5000, 영속화되는 것은 카운터뿐), 덤프·전송 경로가 없다. 설정 파일의 `stat.XX`는 사용 빈도 수준의 메타데이터로 사용자 로컬 ACL 아래에 있다. CVE급 문제 아님.
- **진단 로그**: `%TEMP%` 회전 256KB·1세대라 디스크 고갈 불가, 내용은 시각·지연·재시작뿐(PII 없음). 사전 생성·읽기 공격은 best-effort 실패로 귀결.

### 12.4 문서 불일치(보안 관련)

- 없음. README의 `Ed25519_Signed` 배지, CLAUDE의 "서명·핀고정·이중 해시" 서술, 한계절(UIPI·보안 데스크톱)의 설명이 모두 코드와 일치함을 확인했다.

### 12.5 테스트 증거

- `cargo test --workspace` (exit 0): 197건 전부 통과(4+2+10+170+5+6+0). 보안 관련 포함: 매니페스트 서명·핀·만료·버전 테스트, `helper_compatible` 범위 테스트, 롤백·재실행 테스트, 핫키 파싱 거부 테스트, 불리언·클램프·폴백 저장 테스트.
- `cargo fmt --check`·`clippy -D warnings`: 깨끗.
- 수행하지 않은 것: 실기 UAC·무결성 레벨 교차 메시지 실험, 네트워크 MITM, 퍼징. 같은 이유로 물리적 도달 이슈는 `Likely` 이하로만 분류했다(SEC-001이 해당).

*보안 감사 종료. 코드는 수정하지 않았으며, 본 장은 기존 문서(1~11장)에 추가된 것이다.*

### 12.6 적용 기록 (SEC-001)

사용자 요청으로 12.2의 SEC-001 중 `WM_UPDATE_EXIT` 부분을 구현했다(테스트 포함, 게이트 통과).

- 기동 시 64비트 nonce 생성(`BCryptGenRandom`, 불가 시 OS 시드 폴백, 0 방지) → `on_create`에서 1회 발행.
- 발송처 2곳(업데이트 워커·승격 재실행)이 `wParam`에 nonce 동봉, 핸들러가 불일치 시 `DefWindowProcW`로 무시. 기존 `WPARAM(0)` 발송은 모두 교체해 회귀 없음(승격 경로 포함).
- 테스트: `update_exit_needs_this_process_nonce`(일치·0·오답·미발행 상태). 전체 198건 통과.
- `Win32_Security_Cryptography` feature 1줄 추가(컴파일 타임만, 의존성 버전 불변).

`MENU_EXIT` 확인 대화상자는 적용하지 않았다. 사유: (1) 업데이트·승격 자동 종료 경로는 `MENU_EXIT`을 쓰지 않아 기능과 무관하고, (2) 종료마다 모달을 띄우는 것은 Low 등급에 비해 UX 회귀가 크며, (3) `WM_COMMAND` 잔류 위험은 UIPI(`WM_APP+` 차단 유지) + 동일 사용자 신뢰 경계로 수용한다는 12.2의 판단이 그대로 유효하다. 재검토 조건: 저무결성 프로세스의 메뉴 발송이 실제 악용된 사례가 나오면 그때 다룬다.


