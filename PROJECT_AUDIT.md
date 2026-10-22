# Project Audit

- **대상:** `keyboard-chattering-fix` (`kcfix-rs`). 메인 바이너리와 `crates/kcfix-updater` 헬퍼로 이루어진 Cargo 워크스페이스.
- **기준 시점:** 2026-10-22, HEAD `e67c856` (v0.3.3).
  - 같은 날 작업한 **커밋 전 자동 실행 변경분**도 포함했다: `src/config/autostart.rs`, `src/app/instance.rs`, `src/app/bootstrap.rs`, `src/ui/elevation.rs` 등 14개 파일.
  - 그래서 이 문서의 `autostart`·`instance` 관련 줄 번호는 작업 트리 기준이다.
- **방법:**
  1. 문서 확인
  2. CodeGraph CLI 탐색: 인덱스를 `codegraph sync`로 작업 트리에 맞춘 뒤 `explore`·`callers` 사용
  3. 소스 직접 열람
  4. 게이트·테스트 실행
  - **코드는 수정하지 않았다.**
- **이전 문서와의 관계:** 이 문서는 2026-10-18~19 작성의 이전 `PROJECT_AUDIT.md`를 대체한다. 그 문서는 감사 본문과 적용 기록(10~12장)을 포함하며, 마지막 수정은 커밋 `1c6fed5`이다. 이전 이슈의 현재 상태는 1장 끝의 표에 다시 확인해 두었다.

---

## 1. Executive Summary

### 전체 상태

핵심 경로는 정확하고 방어적으로 짜여 있다.
- **필터 코어:** `Debouncer::decide_event`는 이벤트당 O(1)이다.
- **훅:** 전용 스레드에서 돌며, 세대 번호로 재시작 시 상태를 넘긴다.
- **설정 저장:** tmp → `.bak` → rename 순서로 원자적으로 저장한다.
- **업데이트:** 서명 → 핀 고정 URL → 크기·이중 해시 → 롤백 → 준비 확인 핸드셰이크로 이어진다.

게이트(`fmt`·`clippy -D warnings`·`check --workspace`)와 `cargo test --workspace` 209건이 모두 통과했다.

이번에 남긴 문제는 다섯 가지다.
- **좁은 조건에서 생기는 데이터 유실:** 설정 파일이 손상된 경우.
- **문서와 파서의 어긋남:** 설정이 잘못 해석된다.
- **관리자 권한 경로의 신뢰 경계 문제.**
- **워치독 오탐.**
- **저장 경로 하나의 규약 위반.**

### 전체 위험도: Medium

- Critical은 없다.
- 실사용자에게 닿을 수 있는 **데이터 유실 경로 1건**과 **설정 오해석 1건**이 Medium이다.

### 가장 중요한 문제

1. **[ISSUE-001] README 설정 샘플의 줄 끝 주석을 파서가 값으로 읽는다** (Medium / Confirmed)
   - 샘플대로 `bFilterMouse=0   # ...`처럼 쓰면 **켜짐**으로 해석된다(`v != "0"`).
   - 숫자 항목과 `key.XX`는 조용히 무시된다.
2. **[ISSUE-002] 손상된 본 설정 파일이 기본값으로 로드되고, 다음 저장이 정상 `.bak`까지 덮어쓴다** (Medium / Likely)
   - 정전이나 블루스크린 직후에 NUL로 채워진 파일이 생기면 설정·누적 통계·키별 기준이 영구 유실된다.
   - 저장 시 `fsync`가 없고, "비어 있음"만 손상으로 판정하기 때문이다.
3. **[ISSUE-003] 관리자 권한 경로가 일반 권한으로 쓸 수 있는 파일을 관리자 권한으로 실행·설치한다** (Medium / Confirmed)
   - 경로 1: 로그인 작업이 사용자 폴더(Scoop·winget·포터블)의 exe를 매 로그인마다 관리자 권한으로 실행한다.
   - 경로 2: 관리자 인스턴스에서 업데이트하면 `%TEMP%`의 헬퍼·교체 파일을 관리자 권한으로 실행·설치한다.
4. **[ISSUE-004] 관리자 권한 창이 전경일 때 워치독이 정상 훅을 약 11초마다 재시작한다** (Low / Likely)
   - 홈 화면에 "키보드 감시가 멈췄다"는 잘못된 안내가 남는다.
5. **[ISSUE-005] `on_save`만 `config::load()`(실패 시 기본값)를 기준으로 삼는다** (Low / Confirmed 코드 경로)
   - 저장하는 순간 설정 파일을 읽을 수 없으면 키별 기준이 전부 지워져 라이브 필터에도 적용된다.

### 데이터 손상·유실 가능성

있다. 다만 조건이 좁다.
- **ISSUE-002:** 쓰기 직후 강제 전원 차단 같은 환경 사고에 의존한다.
- **ISSUE-005:** 저장 순간에 본 파일과 백업이 모두 읽기 실패해야 한다.

정상 운영(정상 종료·세션 종료·저장 실패 알림)에서는 유실 경로를 찾지 못했다. 시스템이나 사용자 문서를 파괴하는 경로는 없다.

### 가장 먼저 수정할 영역

1. `src/config/storage.rs`: `save`의 `sync_all`, `try_load`의 "인식 불가 파일" 폴백, 깨진 본 파일을 `.bak`으로 복사하지 않기.
2. 같은 파일의 `parse` 줄 끝 주석 처리, 또는 README 샘플 수정.
3. `src/ui/watchdog.rs`: 관리자 창이 전경이면 프로브 생략.

### 이전 감사 이슈 재검증

| 이전 이슈 | 현재 상태 | 근거 |
|---|---|---|
| ISSUE-001 워치독 재시작 시 두 훅 공존 | 해소 | `HOOK_GEN` 세대 retire/claim (`src/input/hook.rs:466`, `560-563`, `637-639`), 테스트 `restart_hands_over_pending_releases_to_one_thread`·`concurrent_restarts_leave_one_hook_thread` 통과 |
| ISSUE-002 청소 모드와 보류 뗌 | 해소 | `set_cleaning(true)` → `flush_deferred_releases()` (`src/ui/commands.rs:71-75`) |
| ISSUE-003 `bMinimizeOnLaunch` 불리언 규칙 | 해소 | `v != "0"` 통일 (`src/config/storage.rs:119`) |
| ISSUE-004 스테이징 청소가 진행 중인 업데이트 파일 삭제 | 해소 | `STAGING_MAX_AGE` 1시간 (`src/updater/staging.rs:332-366`) |
| PERF-001 해시가 파일 전체를 RAM에 적재 | 해소 | 64KB 스트리밍 (`src/updater/http.rs:26-41`, `crates/kcfix-updater/src/lib.rs:115-128`) |
| SEC-001 인증 없는 `WM_UPDATE_EXIT` | 해소 | nonce 검증 (`src/ui/window.rs:69-73`, `677-688`) |
| SEC-002 뮤텍스 선점(관찰) | 관찰 유지 | 동일 사용자 코드 실행이 전제. 별도로, 이번 세션 변경분이 "관리자 인스턴스가 쥔 뮤텍스의 `ACCESS_DENIED`를 fail-open으로 처리해 훅이 2개 뜨던" 실제 버그를 고쳤다 (`src/app/instance.rs` `held_by_elevated`) |

---

## 2. Project Understanding

### 프로젝트 목적

- Windows 저수준 키보드 훅(`WH_KEYBOARD_LL`)으로 같은 키가 기준 시간 안에 다시 눌리는 **채터링을 차단**하는 상주형 트레이 앱이다.
- 선택 기능도 같은 필터를 쓴다.
  - 마우스 버튼 필터(`WH_MOUSE_LL`)
  - 떼기 확인(`iReleaseDeferMs`)
  - 전경 앱 자동 일시 중지
  - 청소 모드
  - 키별 기준과 권장값
- 서명된 인앱 업데이트를 지원한다.
- 언어는 한국어·영어다.

### 주요 entrypoint

| Entry | 위치 | 역할 |
|---|---|---|
| `main` | `src/main.rs:30-35` | `cli::handle_cli()` → `bootstrap::run(config::load())` |
| CLI | `src/app/cli.rs:16-71` | `--version`/`-V`, `--check-language`, `--check-helper`, `--check-update` (GUI 없이 종료) |
| GUI 기동 | `src/app/bootstrap.rs:84-` | `init_shared` → (`--after-pid` 대기) → (자동 실행 시 관리자 인스턴스 대기) → 단일 인스턴스 → 훅 스레드 → 창 → 메시지 루프 |
| 훅 콜백 | `src/input/hook.rs:171` `hook_proc`, `:254` `mouse_proc` | 전용 스레드의 LL 훅 |
| 창 프로시저 | `src/ui/window.rs:560` `wndproc` | 500ms 타이머, 명령, 세션 종료, 업데이트 종료 |
| 업데이트 헬퍼 | `crates/kcfix-updater/src/main.rs`, `lib.rs:262` `update_executable_with` | 프로세스 종료 대기 → 재해시 → 교체 → 재실행 → 실패 시 롤백 |

### 핵심 모듈

- `debounce`: 판정 코어. `Decision::{Pass, Block, Defer, Flush}`.
- `input::hook`: 훅·인젝터 스레드, 재시작.
- `config`: 설정 모델, INI 원자 저장, 자동 실행(Run·StartupApproved·작업 스케줄러 COM), 사운드 경로 검증.
- `ui`: Win32 창 전반.
  - `settings_form`: 저장
  - `watchdog`: 훅 감시
  - `foreground`: 자동 일시 중지·관리자 창 감지
  - `elevation`: 관리자 재실행·로그인 작업
  - `update`: 업데이트 워커
- `updater`: 매니페스트 검증, 다운로드, 스테이징.

### 데이터 저장 방식

| 저장소 | 내용 | 쓰는 곳 |
|---|---|---|
| `%APPDATA%\KeyboardChatteringFix\settings.ini`(또는 exe 옆 포터블) | 설정 20여 개 필드, `key.XX`(키별 기준, 파일이 원본), `stat.XX`, 누적 카운터 | `config::save`. 호출자: `persist_with`(`settings_form.rs:253`), `on_save`(`:513`), `files::import_settings`(`files.rs:150`) |
| `settings.ini.bak` / `.tmp` | 직전 세대 / 스테이징 | `save` 내부 |
| `HKCU\...\Run`, `...\Explorer\StartupApproved\Run` | 자동 실행 | `config::autostart` |
| 작업 스케줄러 `\KeyboardChatteringFix`, `HKCU\Software\KeyboardChatteringFix\ElevatedTask` | 관리자 로그인 시작과 그 표식 | `config::autostart::set_elevated_task`·`refresh_elevated_task` |
| `%TEMP%` | 업데이트 스테이징, `kcfix-diag.log`(256KB 회전) | `updater::staging`, `ui::diag` |

DB는 없다.

### 외부 의존성

- **크레이트:** `windows 0.62`, `ureq 3`(TLS), `ed25519-dalek`, `sha2`, `serde_json`, `tracing`. 헬퍼 쪽은 `clap`, `hex`, `thiserror`.
- **외부 서비스:** GitHub Releases 매니페스트와 아티팩트.
- **OS:** Windows 전용(설계 의도). LL 훅, `SendInput`, 레지스트리, 작업 스케줄러 COM, `ShellExecuteW`.

### 핵심 실행 흐름

```
[키 입력] OS → hook_proc(전용 스레드) → SHARED 락 → Debouncer::decide_event
          → apply: Pass(CallNextHookEx) / Block(LRESULT 1 + 사운드)
                   / Defer(pending_up + 스레드 타이머) → drain → injector 스레드 SendInput(INJECT_MARK)

[설정 저장] 저장 버튼 → settings_form::on_save → (폼 값 + config::load()) → SHARED에 적용
          → config::save(tmp → .bak 복사 → rename) → 실패 시 msg_box

[주기 저장] WM_TIMER 500ms → autosave_tick(5분) → persist_live → persist_with
          → config::try_load로 key.XX 병합 → 라이브 카운터 → config::save

[업데이트] 메뉴 → spawn_update_check(워커) → check_for_update(서명 검증) → 확인 대화상자
          → prepare_update(헬퍼 %TEMP% 복사 + 다운로드·해시) → launch_helper → WM_UPDATE_EXIT(nonce)
          → 헬퍼: 종료 대기 → 재해시 → rename 교체 → 재실행·준비 확인 → 실패 시 롤백 + 이전 버전 재실행

[자동 실행] 로그인 → (Run) 일반 --autostart / (작업) 관리자 --autostart
          → bootstrap: 표식 있고 일반이면 관리자 인스턴스 최대 20초 대기 → 단일 인스턴스
```

---

## 3. Audit Coverage & Limitations

### 실제 확인한 주요 모듈 (전체 열람)

- 입력·필터: `src/input/hook.rs`, `src/debounce.rs`(판정부)
- 설정·자동 실행: `src/config/storage.rs`, `src/config/autostart.rs`, `src/config/sound.rs`
- 앱 골격: `src/app/{bootstrap,instance,cli}.rs`, `src/main.rs`
- UI: `src/ui/{settings_form,commands,window(주요 부분),watchdog,foreground,elevation,update,files,hotkeys(파서),keys_page(변경 경로)}.rs`
- 업데이트: `src/updater/{http,staging}.rs`, `crates/kcfix-updater/src/lib.rs`(교체·롤백·준비 확인)
- 문서: `README.md`(설정 샘플·한계 절), `docs/UPDATE.md`(실행 인자 절), `CLAUDE.md`, `GEMINI.md`(grep)

### CodeGraph로 분석한 호출 관계

- **환경:** MCP 서버는 연결 실패 상태였다. 그래서 셸 CLI(`codegraph`)를 썼다.
  - 인덱스가 작업 트리보다 뒤처져 있어(변경 46개 파일) `codegraph sync`로 갱신한 뒤 사용했다.
- `explore "hook_proc mouse_proc decide_event drain due_releases injector restart"`
  - `decide_event`의 호출자 5개(hook·debounce)와 `due_releases`의 호출자 3개를 확인했다.
  - 훅 → apply → 인젝터 경로를 확인했다.
- `callers`
  - `restart` ← `watchdog::tick` ← `wndproc`
  - `register_task` ← `set_elevated_task`·`refresh_elevated_task`
  - `try_load` ← `load`

### 실행한 테스트

| 명령 | 결과 |
|---|---|
| `cargo fmt --all -- --check` | 통과(exit 0) |
| `cargo clippy --all-targets --all-features -- -D warnings` | 통과(exit 0) |
| `cargo check --workspace --all-targets` | 통과 |
| `cargo test --workspace` | **209건 전부 통과** (헬퍼 lib 4 + 헬퍼 main 2 + 헬퍼 통합 10 + 앱 단위 178 + `edition_2024` 4 + `exe_smoke` 5 + `version_sync` 6). 실제 LL 훅 테스트도 건너뛰지 않고 실행됨 |

### 확인하지 못한 환경·외부 서비스

- 관리자 권한(UAC) 실기 경로: 관리자 인스턴스, 무결성 수준 교차 `SendInput`·뮤텍스, 로그인 작업의 실제 기동. 감사 세션이 중간 무결성(비관리자)이었다.
  - ISSUE-003과 ISSUE-004를 Confirmed/Likely로 나눈 근거가 이것이다.
- 전원 차단·블루스크린 상황의 파일 시스템 동작(ISSUE-002).
- GitHub 실서버 업데이트 왕복, 프록시 환경.
- GUI E2E 스크립트(`scripts/e2e/*.ps1`). 프로젝트 규칙상 테스트에서 GUI를 띄우지 않으며, 이번 감사에서도 실행하지 않았다.
- Python 매니페스트 스크립트 테스트(`scripts/test_update_manifest.py`). 실행하지 않았다.

### CodeGraph와 분석의 한계

- **`pub use` 허브 재수출 경로를 해석하지 못한다.** `config::load`, `crate::updater::prepare_update` 같은 경로다. 그래서 `callers load`가 테스트만 보여 줬다.
  - 실제 UI 호출자는 grep으로 확정했다: `settings_form.rs:462`, `main.rs:34`, `staging.rs`의 호출자인 `update.rs:109`.
- `explore` 출력은 파일 일부를 생략한다. 핵심 파일은 직접 전체를 열람해 보완했다.

---

## 4. High-Risk Issues

> 심각도 기준은 지시서를 따른다. 이 장은 Confirmed와 근거가 강한 Likely만 담는다.

### [ISSUE-001] README 설정 샘플의 줄 끝 주석이 값으로 읽혀 불리언이 켜지고 숫자·키별 값이 무시된다

- **위치:**
  - `src/config/storage.rs`: `parse` (`83-206`). 특히 `91-92` `split_once('=')` + `trim`, 불리언 `104-162`의 `v != "0"`, 숫자 `parse::<u64>()`, `key.XX` `198-204`.
  - `README.md:122-148`(영문), `README.md:331-357`(한글) 설정 샘플.
- **우선순위:** Medium
- **신뢰도:** Confirmed (코드 흐름)
- **문제:** 파서는 `#`·`;`로 **시작하는** 줄만 주석으로 건너뛴다(`88`). 그런데 README는 `iThreshold=50          # Global chatter threshold...`처럼 **줄 끝 주석**을 단 샘플을 "설정 파일은 이렇게 저장된다"고 제시한다. 이렇게 적으면 값이 다음처럼 해석된다.
  - 불리언: `"0           # Play notification..."`은 `"0"`이 아니므로 **참**이 된다.
  - 숫자와 `key.XX`: `parse::<u64>()`가 실패해 **조용히 무시**된다.
  - `lpSoundFile=   # Path...`: 사운드 경로가 `# Path to custom .wav file ...`이 된다(검증에 실패해 시스템음으로 폴백).
- **발생 조건:** 사용자가 README 샘플을 복사하거나 같은 양식으로 `settings.ini`를 직접 편집할 때. README 150행은 파일 직접 편집을 권장하는 문맥이다.
- **영향:**
  - `bPlaySound=0 #…` → 차단음 켜짐.
  - `bFilterMouse=0 #…` → **마우스 버튼 필터 켜짐**. 빠른 더블클릭이 채터링으로 차단될 수 있다.
  - `bAutoDisableFullscreen=0 #…` → 전체 화면 앱에서 보호 자동 꺼짐.
  - `bMinimizeOnLaunch=0 #…` → 트레이로 시작.
  - `key.48=120 #…` 같은 키별 기준은 적용되지 않는다.
  - 어떤 경고도 없다. UI에서 고치면 정상 값으로 다시 저장되므로 복구는 가능하다.
- **근거:** `storage.rs:91-92`는 값을 그대로 `trim`만 한다. `104`, `107`, `119`, `151`, `162`는 `v != "0"`, `200`은 `v.parse::<u64>()`를 쓴다. 주석이 섞인 값을 다루는 테스트는 없다(`storage.rs` 테스트 전체 확인).
- **반증 확인:**
  - 프로그램 자신이 쓰는 파일에는 주석이 없다(`save` `216-257`). 그래서 **수동 편집에서만** 발생한다.
  - 가져오기(`load_from`)도 같은 `parse`를 쓰므로 막아 주지 못한다.
  - 언어(`from_setting`)는 알 수 없는 값을 `Auto`로 폴백해 무해하다. 단축키는 파싱에 실패하면 무시하고, 저장할 때만 경고한다.
  - 즉 보호 장치가 일부 필드에만 있다.
- **호출·영향 범위:** `parse` ← `try_load`/`load`/`load_from` ← `main.rs:34`, `persist_with`(`settings_form.rs:257`), `on_save`(`:462`), `import_settings`(`files.rs:137`). 설정을 읽는 모든 경로에 해당한다.
- **권장 수정 방향:** 둘 중 하나를 고른다.
  - (a) README 샘플의 주석을 별도 줄(`; ...`)로 옮긴다. 비용이 가장 낮다.
  - (b) `parse`에서 공백 뒤 `#`·`;` 이후를 잘라낸다. 사운드 경로에 `#`이 있을 수 있으니 "공백 + `#`" 규칙으로 하거나 `lpSoundFile`은 예외로 둔다.
  - 둘 다 하는 편이 가장 안전하다.
- **필요한 회귀 테스트:**
  - `"[KeyboardChatteringFix]\nbFilterMouse=0   # c\niThreshold=80 # c\nkey.48=120 # c\n"`을 `load()`하면 `filter_mouse == false`, `threshold_ms == 80`, `per_key_ms[0x48] == Some(120)`.
  - README 샘플 블록 자체를 `include_str!`로 읽어 파싱 결과가 주석의 기본값과 일치하는지 검사한다(문서 회귀 방지).

### [ISSUE-002] 손상된 본 설정 파일이 기본값으로 로드되고, 다음 저장이 정상 `.bak`까지 덮어써 설정·통계가 영구 유실된다

- **위치:**
  - `src/config/storage.rs`: `save` (`211-269`; `262` `fs::write`, `266` `fs::copy(path → .bak)`, `268` `rename`), `try_load` (`70-81`), `parse` (`83-206`).
  - 호출자 `src/main.rs:34`, `src/ui/settings_form.rs:253-281` `persist_with`.
- **우선순위:** Medium
- **신뢰도:** Likely. 코드 경로는 확정이고, 트리거(전원 차단 후 NUL 파일)는 환경 재현을 하지 않았다.
- **문제:** 세 단계가 겹쳐 유실이 일어난다.
  1. `save`는 tmp에 쓴 뒤 `sync_all`(FlushFileBuffers) 없이 rename한다. NTFS는 메타데이터(rename·크기)만 저널링한다. 그래서 쓰기 직후 강제 전원 차단이나 블루스크린이 나면 **크기는 맞고 내용은 NUL인** 파일이 남을 수 있다(여러 앱에서 흔히 보고되는 현상).
  2. `try_load`는 본 파일이 **읽기 실패이거나 공백뿐일 때만** `.bak`으로 폴백한다(`73`). NUL은 공백이 아니므로 `parse`가 인식하지 못한 줄을 전부 버리고 `Settings::default()`를 돌려준다.
  3. 다음 저장(5분 자동 저장·종료·키 탭 변경)의 흐름은 이렇다.
     - `persist_with`가 디스크(=기본값)에서 `key.XX`를 받아 키별 기준을 지운다(`adopt_file_only_fields`).
     - `save`가 손상된 본 파일을 `.bak`으로 **복사**한다(`266`). 이때 정상이던 직전 세대가 덮어써진다.
- **발생 조건:** 설정 쓰기 직후 몇 초 안에 정전, 강제 전원 버튼, 블루스크린이 일어나는 경우.
  - 쓰기는 5분 자동 저장, 종료, 세션 종료, 키 탭 변경, 통계 초기화, 언어 변경 때 일어난다.
  - 정상 종료는 캐시를 플러시하므로 해당하지 않는다.
- **영향:** 기준값·키별 기준·단축키·제외 앱 목록·누적 통계가 전부 기본값으로 돌아간다. 복구할 수 있는 세대(`.bak`)도 없어진다. 사용자는 사라졌다는 사실만 알 수 있다.
- **근거:**
  - `storage.rs:262-268`에 `sync_all`과 `MOVEFILE_WRITE_THROUGH`가 없다.
  - `73`은 `!text.trim().is_empty()`만 검사한다. `parse`는 헤더나 인식 키가 하나도 없어도 `Some(defaults)`가 된다.
  - `266`은 본 파일의 유효성을 확인하지 않고 복사한다.
- **반증 확인:**
  - 빈 파일 폴백은 있다. 테스트 `storage.rs:489-491`이 빈 본 파일에서 `.bak`을 읽는 것을 확인한다. 그러나 NUL이나 쓰레기 내용은 막지 못한다.
  - 통계는 메모리에서 다시 채워질 수 있다는 반론이 가능하다. 그러나 기동 시 기본값(0)으로 시작하므로 근거가 되지 않는다.
  - `on_save`도 `config::load()`를 쓰므로 같은 결과가 된다.
- **호출·영향 범위:** `save` ← `persist_with`(종료 `window.rs:411`, 세션 종료 `:655`, 자동 저장 `settings_form.rs:322`, 통계 초기화 `commands.rs:158`, 언어 `settings_form.rs:356/360`, 내보내기 `files.rs:122`, 키 탭 `keys_page.rs:449/460/474/481`), `on_save`(`:513`), 가져오기(`files.rs:150`).
- **권장 수정 방향:**
  1. tmp에 쓴 뒤 `File::sync_all()`을 하고 rename한다. 가능하면 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`를 쓴다.
  2. `try_load`에서 "헤더나 알려진 키가 하나도 없는" 본 파일도 손상으로 보고 `.bak`으로 폴백한다.
  3. `save`의 `.bak` 복사는 본 파일이 파싱 가능할 때만 한다.
  4. 선택: 폴백이 일어나면 `diag::log_line`에 기록한다.
- **필요한 회귀 테스트:**
  - 본 파일을 `vec![0u8; 512]`로, `.bak`을 `iThreshold=111`로 두면 `load().threshold_ms == 111`.
  - 그 상태에서 `save(&s)`를 하면 `.bak`이 여전히 `iThreshold=111`을 담는다.
  - `"hello\n"`처럼 헤더가 없는 본 파일도 같은 폴백을 탄다.
  - 기존 `with_isolated_appdata` 패턴(`storage.rs:279-296`)을 사용한다.

### [ISSUE-003] 관리자 권한 경로가 일반 권한 프로세스가 쓸 수 있는 파일을 관리자 권한으로 실행·설치한다

- **위치:**
  - `src/config/autostart.rs`: `register_task` (`404-426`, `current_exe` 경로를 그대로 `HighestAvailable` 작업에 등록).
  - `src/updater/staging.rs`: `stage_helper` (`226-231`), `prepare_update` (`233-251`), `launch_helper` (`289-306`).
  - `crates/kcfix-updater/src/lib.rs`: 재해시 `292-303` → rename `320`.
- **우선순위:** Medium
- **신뢰도:** Confirmed. 경로 권한 검사가 없다는 것을 코드로 확인했다. 악용 실증은 하지 않았다.
- **문제:** 두 갈래다.
  - **(a) 로그인 작업:** 작업은 등록 시점의 exe를 **매 로그인마다 관리자 권한으로** 실행한다. 이 exe가 사용자가 쓸 수 있는 폴더에 있으면, 같은 사용자의 **일반 권한 프로세스가 exe를 바꿔치기해 다음 로그인 때 관리자 권한을 얻는다.** 일반적인 배포 위치는 모두 사용자 쓰기 가능이다.
    - Scoop: `%USERPROFILE%\scoop\apps\…`
    - winget 포터블: `%LOCALAPPDATA%\Microsoft\WinGet\Packages\…`
    - 다운로드 폴더 포터블
  - **(b) 업데이트:** 관리자 인스턴스에서 업데이트하면 다음 순서로 진행된다.
    1. 헬퍼를 `%TEMP%`에 복사한다(`227`).
    2. **다운로드가 끝날 때까지(수 초~수 분)** 그대로 둔 뒤, 관리자 토큰을 상속한 채 실행한다(`303`).
    3. 교체 파일도 `%TEMP%`에 있다. 헬퍼가 재해시한 뒤 rename하기까지 틈이 있다.
    - 즉 일반 권한 프로세스가 스테이징 파일을 바꾸면 관리자 권한 코드 실행이 된다.
- **발생 조건:** 사용자 계정에서 이미 악성 코드가 실행 중이고, 다음 중 하나에 해당할 때.
  - (a) 관리자 로그인 작업을 켰다.
  - (b) 관리자 권한 인스턴스에서 업데이트를 실행했다. 패키지 관리자 설치본은 안내만 하므로 해당하지 않는다(`update.rs:86-97`).
- **영향:** 일반 권한에서 관리자 권한으로 승격된다. (a)는 영속적이다.
- **근거:**
  - `register_task`는 `current_exe()`를 그대로 `<Command>`에 넣고, 경로의 ACL이나 위치를 검사하지 않는다.
  - `stage_helper`는 `std::env::temp_dir()`(사용자 쓰기 가능)에 복사한다.
  - `Command::spawn`은 부모 토큰을 상속한다.
  - 이번 세션의 미커밋 변경은 작업 정의 XML의 `%TEMP%` 경유만 없앴다(COM 등록). 실행 대상 파일 자체의 위치 문제는 남아 있다.
- **반증 확인:**
  - Microsoft는 UAC를 보안 경계로 보지 않는다. 동일 사용자의 악성 코드가 전제다. 그래서 Critical이나 High가 아닌 Medium으로 분류했다.
  - 그래도 (a)는 "한 번 켜 두면 매 로그인 무프롬프트 승격"이라는 지속성이 있고, 이 기능 자체가 만든 경로다.
  - (b)의 해시 검증은 다운로드 손상과 위조는 막지만, 검증 후 바꿔치기(TOCTOU)는 막지 못한다.
- **호출·영향 범위:**
  - (a) `elevation::toggle_task` → `set_elevated_task(true)` → `register_task`. `refresh_elevated_task`(관리자 기동 시 재등록)도 같은 경로를 쓴다.
  - (b) 메뉴 `MENU_UPDATE` → `spawn_update_check` → `prepare_update` → `launch_helper`.
- **권장 수정 방향:**
  - (a) 다음 중 하나 이상을 적용한다.
    - 작업을 켤 때 exe 경로가 관리자 전용 쓰기 위치(`Program Files` 등)인지 확인하고, 아니면 경고하거나 거부한다.
    - README와 메뉴 문구에 위험을 명시한다.
  - (b) 관리자 인스턴스에서는 인앱 업데이트를 막고 "일반 권한으로 다시 실행 후 업데이트"를 안내한다. 또는 스테이징 디렉터리를 관리자 전용 ACL로 만들고, 헬퍼를 잠긴 핸들로 실행한다.
- **필요한 회귀 테스트:**
  - 순수 함수 `is_admin_only_location(path)`를 만들어 단위 테스트한다. 기대값은 `C:\Program Files\X\a.exe`가 true, `%LOCALAPPDATA%\…`가 false.
  - 관리자 인스턴스의 업데이트 분기를 순수 판정으로 떼어 내 "elevated → 업데이트 차단 안내"를 검사한다.

### [ISSUE-004] 관리자 권한 창이 전경일 때 워치독이 정상 훅을 약 11초마다 재시작하고 잘못된 안내를 남긴다

- **위치:**
  - `src/ui/watchdog.rs`: `tick` (`116-165`), `send_probe` (`97-113`), `check` (`43-49`).
  - 호출 `src/ui/window.rs:640`, 안내 `src/ui/stats.rs:132`. 전경 관리자 감지는 `src/ui/foreground.rs:239`에 있지만 워치독은 쓰지 않는다.
- **우선순위:** Low
- **신뢰도:** Likely. 코드와 OS 문서 근거가 있고, 관리자 환경 실측은 하지 않았다.
- **문제:** 앱이 일반 권한이고 관리자 권한 창(작업 관리자, 관리자 터미널)이 전경이면 다음처럼 흘러간다.
  1. 그 창으로 가는 입력을 훅이 보지 못한다(README `165`행이 명시한 UIPI 한계). 그래서 `HOOK_EVENTS` 증가가 0이다.
  2. `GetLastInputInfo`는 세션 전체 입력(마우스 포함)이라 "사용자 활동 중"이 된다. 그래서 `check`가 `Probe`를 반환한다.
  3. 프로브 `SendInput`은 UIPI로 상위 무결성 전경에 주입되지 못한다. MSDN상 반환값으로 실패를 알 수 없다고 하므로 `send_probe`는 true를 반환하고 대기를 건다.
  4. 1초 뒤 `probe_failed`로 `restart()`가 호출된다.
- **발생 조건:** 비관리자 실행 중에 관리자 창이 전경에 있고 10초 이상 키보드나 마우스를 쓰는 경우. 흔한 상황이다.
- **영향:**
  - 훅 스레드가 주기적으로 교체된다. 비용이 작고, 세대 인계로 상태 손실은 없다.
  - `%TEMP%\kcfix-diag.log`가 오탐 재시작 기록으로 채워진다. 정작 실제 장애를 추적할 때 방해가 된다.
  - 관리자 창을 벗어나면 홈 화면에 **"{시각}에 키보드 감시가 멈춰 자동으로 다시 시작했어요"**가 표시된다(`stats.rs:132`). 사용자는 앱이 불안정하다고 오해한다.
- **근거:** `window.rs:640`이 매 틱 무조건 `watchdog::tick()`을 호출한다. `tick`은 전경 상태를 보지 않는다. `foreground::status()`의 두 번째 값(관리자 창 전경 여부)을 쓸 수 있는데도 쓰지 않는다.
- **반증 확인:**
  - `send_probe`의 반환값 검사(이전 감사의 ISSUE-001 수정)는 보안 데스크톱처럼 `SendInput`이 0을 반환하는 경우만 거른다. UIPI의 "조용한 차단"은 거르지 못한다.
  - 관리자 권한으로 실행 중이면 이 문제는 생기지 않는다(`self_elevated`).
- **호출·영향 범위:** `wndproc(WM_TIMER)` → `watchdog::tick` → `hook::restart` → `start`/`abandon`. 표시 쪽은 `stats::render_stats`.
- **권장 수정 방향:** `foreground::status().1`(관리자 창이 전경이고 우리는 비관리자)이 참인 동안에는 검사를 건너뛰고 기준값(`st.hooked`)만 갱신한다. 또는 `check`에 `elevated_foreground` 인자를 추가해 `Fine`으로 처리한다.
- **필요한 회귀 테스트:**
  - `check(0, true, elevated_foreground = true) == Step::Fine`
  - `check(0, true, false) == Step::Probe`
  - 기존 `probe_only_when_the_user_was_active_and_the_hook_silent`를 확장한다.

### [ISSUE-005] `on_save`만 `config::load()`를 기준으로 삼아, 파일을 읽을 수 없는 순간 저장하면 키별 기준이 전부 지워진다

- **위치:** `src/ui/settings_form.rs`: `on_save` (`462` `config::load()`, `485-487` 라이브 적용, `513` 저장). 비교 대상은 `persist_with` (`257` `config::try_load()`).
- **우선순위:** Low
- **신뢰도:** Confirmed (코드 경로). 트리거는 드물다.
- **문제:**
  - `storage.rs:67-69`의 문서는 "디스크 상태를 메모리에 병합하는 호출자는 `try_load`를 써서, 읽을 수 없는 파일이 '전부 삭제됨'으로 오인되지 않게 한다"고 규약을 정해 둔다.
  - `persist_with`는 이 규약을 지킨다. `on_save`는 `load()`를 써서, 본 파일과 `.bak`이 모두 읽기 실패이면 `Settings::default()`(키별 기준이 모두 `None`)를 기준으로 삼는다.
  - 그 결과 `485-487`이 라이브 필터의 키별 기준을 모두 지우고, `513`이 그대로 저장한다.
- **발생 조건:** 저장 버튼(또는 가져오기의 `on_save`)을 누르는 순간 본 파일과 `.bak`이 모두 읽기 실패인 경우. 예를 들면 다음과 같다.
  - 동기화나 백업 도구의 배타 잠금
  - 둘 다 UTF-8이 아닌 상태
  - 포터블 폴더 권한 문제
- **영향:** 키별 기준(파일이 원본)이 유실되고 필터 동작이 즉시 바뀐다. 경고는 없고, 저장 성공으로 표시된다.
- **근거:** 코드 직접 확인. CLAUDE.md도 "파일이 per-key 값의 원본이라는 전제를 깨지 말 것"을 철칙으로 둔다.
- **반증 확인:**
  - 정상적으로 읽히는 파일에서는 문제가 없다.
  - 손상(ISSUE-002)이 아닌 순수 읽기 실패는 드물다. 그래서 Low로 분류했다.
  - 다른 호출 경로(`persist_with`)는 이미 안전하다.
- **호출·영향 범위:** `on_save` ← 저장 버튼·Enter(`window.rs:338/380`), `files::import_settings`(`files.rs:155`).
- **권장 수정 방향:** `config::try_load()`가 `None`이면 `SAVED`의 현재 값을 기준으로 삼는다(`persist_with`와 같은 규칙). 또는 저장을 중단하고 `save_failed`를 표시한다.
- **필요한 회귀 테스트:** 기준 설정 선택을 순수 함수 `save_base(disk: Option<Settings>, saved: &Settings) -> Settings`로 떼어 낸다. 기대값은 다음과 같다.
  - `None`이면 `saved.per_key_ms`가 보존된다.
  - `Some(d)`이면 `d.per_key_ms`를 채택한다.

---

## 5. Potential Functional Gaps

버그가 아니라 보완 여지다. 추정은 버그처럼 쓰지 않는다.

- **관리자 시작 표식이 일반 권한에서는 갱신되지 않는다 (Likely Gap, 이번 세션 미커밋 변경).**
  - 작업 스케줄러 UI에서 작업을 지우거나 끄면, 표식(`HKCU\Software\KeyboardChatteringFix\ElevatedTask`)은 관리자 인스턴스가 다음에 기동할 때까지 남는다(`refresh_elevated_task`).
  - 그동안 일반 자동 실행은 **매 로그인 20초** 대기한 뒤 대신 실행된다(`bootstrap.rs:108`, `ELEVATED_START_GRACE`). 보호 공백은 20초로 한정된다.
  - 방향: 대기가 시간 초과로 끝나면 표식을 지운다. 일반 권한도 HKCU는 쓸 수 있다.
- **떼기 확인 재주입 실패를 무시한다 (추정).**
  - `send`(`hook.rs:361-369`)는 `SendInput` 반환값을 버린다.
  - UAC 프롬프트처럼 보안 데스크톱으로 전환되기 직전에 보류된 뗌(최대 100ms)이 전환 뒤에 주입되면 실패할 수 있다. 그러면 해당 키가 시스템상 눌린 상태로 남을 가능성이 있다.
  - `iReleaseDeferMs` 기본값이 0이라 기능을 켠 사용자에게만 해당한다. 실측하지 않았다.
- **자동 실행 시에도 창을 띄운다 (Likely Gap).**
  - `show_initial`(`window.rs:796-800`)은 `--autostart`와 무관하게 `bMinimizeOnLaunch`만 본다(기본 0). 그래서 기본 설정이면 매 로그인마다 메인 창이 뜬다.
  - 의도일 수 있으므로 버그로 분류하지 않았다. 방향: `--autostart`이면 트레이로 시작하는 옵션을 두거나 기본값을 바꾼다.
- **비UTF-8 설정 파일을 조용히 폴백한다 (Likely Gap).**
  - ANSI(CP949)로 저장된 수동 편집 파일은 `read_to_string`에 실패해 `.bak`(이전 세대)으로 폴백한다. 사용자의 편집은 알림 없이 무시되고, 다음 저장에서 `.bak`으로 밀려난다.
  - Windows 11 메모장의 기본값이 UTF-8이라 빈도는 낮다.
- **관리자 재실행 인계의 10초 한도 (추정).**
  - `--after-pid` 대기는 10초다(`bootstrap.rs` `wait_for_process_exit`).
  - 이전 인스턴스 종료가 이보다 오래 걸리면(예: 백신 검사 중인 `persist_live`) 새 관리자 인스턴스는 "이미 실행 중"을 띄우고 끝난다. 그 뒤 이전 인스턴스가 끝나면 보호가 공백이 된다.
- **관리자·UAC 경로의 자동 검증이 없다 (Confirmed Gap).**
  - ISSUE-003·004와 이번 세션의 뮤텍스 `ACCESS_DENIED` 수정은 모두 관리자 실기에서만 확인할 수 있다.
  - `scripts/e2e`에도 해당 시나리오가 없다(`features_e2e`는 떼기 확인·마우스·단축키·일시 중지만 다룬다).
- **프록시 미지원 (추정, 이전 감사에서 이월).** `ureq` 에이전트에 시스템 프록시 설정이 없다(`http.rs:13-22`).
- **조사했으나 채택하지 않은 것:**
  - 일시 중지·보호 꺼짐 중에는 훅이 키 상태를 추적하지 않아 `held`가 남는다. 재개 후 다음 1회 누름이 오토리핏으로 보고 통과되는 fail-open이라 안전하다.
  - 업데이트 다운로드가 중간에 끊긴 경우의 부분 파일은 `prepare_update`가 삭제한다(`staging.rs:239-242`). 반증됐다.
  - `WM_ELEVATED_TASK_DONE` 위조는 `BUSY` 가드로 무시된다.
  - 워커 스레드의 `msg_box`는 동작상 문제가 없다.
  - `InjectQueue` 32칸 초과는 실제 손으로는 도달할 수 없다.

## 6. Documentation Mismatches

- **README 설정 샘플의 줄 끝 주석** (`README.md:122-148`, `331-357`): 실제 파서는 지원하지 않는다. 기능 영향이 있어 ISSUE-001로 올렸다.
- **CLAUDE.md "설정 쓰기는 `persist_live`(종료·세션 종료·5분 자동 저장·통계 초기화·언어 변경)와 `on_save`뿐":** 실제 `persist_live`/`persist_with` 호출처에는 다음도 있다.
  - 설정 내보내기(`files.rs:122`)
  - 키별 통계 지우기(`keys_page.rs:474`)
  - 키 탭 임계값과 마우스 스위치(`settings_form.rs:287-309`, CLAUDE.md의 다른 절에 서술됨)
  - 나열이 불완전할 뿐 동작 위험은 없다.
- **`docs/UPDATE.md:107-109` `--autostart` 설명:** 이번 세션 미커밋 변경으로 생긴 "관리자 작업 표식이 있으면 일반 자동 실행이 관리자 인스턴스를 최대 20초 기다린다"는 동작이 빠져 있다. CHANGELOG·CLAUDE.md에는 반영돼 있다.
- **코드 주석 불일치:** `src/updater/staging.rs:23-28`은 `update_in_progress`를 "API 호환용, 현재 흐름은 쓰지 않음"이라며 `#[allow(dead_code)]`를 달았다. 실제로는 `menu.rs:133`과 `tray.rs:211`이 사용한다. 기능 영향은 없다.
- **`AGENTS.md` 없음:** 지시서가 확인을 요구했지만 저장소에 없다. README·CLAUDE·GEMINI가 역할을 대신한다.
- **이 밖의 확인 결과:** README의 한계 절(UIPI·보안 데스크톱), 포터블 모드, 설정 경로, `key.XX` 즉시 기록 서술, CLAUDE.md의 핫패스 규칙·모듈 지도는 코드와 일치한다.

## 7. Recommended Fix Plan

실제 코드는 수정하지 않았다. 아래는 순서 제안이다.

### Phase 1 — Immediate

- **ISSUE-002:** `save`에 `sync_all`(+ `MOVEFILE_WRITE_THROUGH`)을 넣고, `try_load`의 "인식 불가" 폴백을 추가하고, `.bak` 복사를 유효성 조건부로 바꾼다. 데이터 유실을 막는 유일한 항목이다.
- **ISSUE-001:** README 샘플 주석을 별도 줄로 옮긴다(즉시). 파서의 줄 끝 주석 처리는 Phase 2에서 검토한다.

### Phase 2 — Stability

- **ISSUE-004:** 관리자 창이 전경인 동안 워치독 프로브를 생략한다(1~2줄 + 테스트).
- **ISSUE-005:** `on_save` 기준을 `try_load` 규칙으로 통일한다.
- **ISSUE-001 (b):** `parse`의 "공백 + `#`/`;`" 줄 끝 주석 처리 + 테스트.
- **5장 표식 Gap:** 대기 시간 초과 시 표식을 지운다.

### Phase 3 — Structural

- **ISSUE-003:** 관리자 로그인 작업에 경로 위치 검사·경고를 넣고, 관리자 인스턴스의 인앱 업데이트 정책을 정한다(차단 또는 ACL 스테이징). 위협 모델을 README에 명시한다.
- **관리자·UAC 시나리오 E2E 추가:** 관리자 인스턴스 + 일반 실행 = 1개만 생존, 로그인 작업 실기동, 관리자 창 전경 시 워치독 무재시작.
- **`on_save`를 순수 함수로 분리:** 기준 설정 선택과 폼 → 설정 매핑을 떼어 내 HWND 없이 테스트할 수 있게 한다.

## 8. Test Recommendations

- **Unit**
  - (ISSUE-001) `bFilterMouse=0   # c` / `iThreshold=80 # c` / `key.48=120 # c`를 `load()`하면 `false` / `80` / `Some(120)`.
  - (ISSUE-001) README 샘플 블록을 `include_str!`로 파싱한 결과가 문서의 기본값(`threshold 50`, 불리언 0 항목 거짓, `key.48 = 120`)과 같다.
  - (ISSUE-002) 본 파일이 NUL 512바이트이고 `.bak`이 `iThreshold=111`이면 `load().threshold_ms == 111`.
  - (ISSUE-002) 헤더 없는 `"hello\n"` 본 파일도 `.bak` 값을 돌려준다.
  - (ISSUE-004) `check(0, true, elevated_fg = true) == Fine`, `check(0, true, false) == Probe`.
  - (ISSUE-005) `save_base(None, &saved).per_key_ms == saved.per_key_ms`, `save_base(Some(d), &saved).per_key_ms == d.per_key_ms`.
  - (ISSUE-003) `is_admin_only_location(r"C:\Program Files\K\k.exe") == true`, `%LOCALAPPDATA%` 하위는 `false`.
- **Integration**
  - (ISSUE-002) 격리 APPDATA에서 `save(A)`를 한 뒤, 본 파일을 NUL로 덮고 `save(B)`를 한다. 기대: `.bak`이 A를 유지(손상본으로 덮어쓰지 않음).
  - (자동 실행) 격리 레지스트리 키는 쓸 수 없으므로, `registered_exe`/`approved_says_disabled` 같은 순수 함수를 유지하고 확장한다(현재 있음).
- **End-to-End (수동, 릴리스 전)**
  - 기존 `hook_session_e2e`·`features_e2e`·`update_swap_e2e` 실행. 이번 감사에서는 실행하지 않았다.
  - 신규 관리자 시나리오:
    - 관리자 인스턴스 실행 중 일반 실행 → "이미 실행 중"이 뜨고 훅은 1개.
    - 관리자 작업 + Run 둘 다 켜고 재로그인 → 관리자 인스턴스만 생존.
    - 작업 관리자를 전경에 두고 30초 동안 타이핑 → `kcfix-diag.log`에 재시작 기록 없음(ISSUE-004 수정 후).
- **Concurrency**
  - 키 탭 변경(`set_key_override`)과 자동 저장 틱이 연속으로 실행된 뒤 파일의 `key.XX`가 마지막 변경을 반영한다(UI 스레드 단일이라 순서 보장. 회귀 방지용).
  - 기존 `concurrent_restarts_leave_one_hook_thread`는 유지한다.
- **Regression**
  - 이번 세션의 `running_probe_sees_holder_without_claiming`과 `only_access_denied_means_held_by_elevated`를 유지한다.
  - 기존 범위형 assertion이 있으면 정확한 값으로 좁힌다(프로젝트 철칙).
- **Platform-specific**
  - Windows 전용. DPI 125%/150%, 다크·라이트 테마, 관리자·비관리자, 비UTF-8 설정 파일(CP949) 로드 시 폴백 동작을 수동 체크리스트로 둔다.

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Acceptable | 필터 코어·훅·업데이트 검증은 정확하다. 수동 편집 설정의 오해석(ISSUE-001)과 관리자 창 전경 시 워치독 오탐(ISSUE-004)이 남는다 |
| Runtime Stability | Good | 훅 전용 스레드, 세대 인계, poison 내성, 락 밖 Win32 호출, 재진입 가드. 오탐 재시작도 상태를 잃지 않는다 |
| Data Integrity | Acceptable | 원자 저장과 `.bak`은 있다. 다만 `fsync`가 없고 손상 판정이 "빈 파일"뿐이라 전원 사고 시 백업까지 잃을 수 있다(ISSUE-002). `on_save`의 규약 위반도 있다(ISSUE-005) |
| Error Resilience | Good | 저장·자동 실행·작업·업데이트·헬퍼 실패마다 사용자 알림이나 롤백이 있다. 업데이트 실패 시 이전 버전을 재실행한다 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도이며 문서와 일치한다. Windows 안에서는 인코딩 폴백(비UTF-8)과 관리자·비관리자 교차 동작에 빈틈이 있다 |
| Test Confidence | Acceptable | 209건 통과, 경계·폴백·동시성 테스트가 풍부하다. 다만 이번 이슈 5건 중 테스트로 잡히던 것이 없고, 관리자·UAC 경로와 GUI E2E는 자동 검증 밖이다 |

**실제로 먼저 수정할 문제 3개**

1. **ISSUE-002:** 설정 저장의 `sync_all`, 손상 파일 폴백, `.bak` 보호. 유일한 영구 데이터 유실 경로다.
2. **ISSUE-001:** README 샘플을 즉시 고치고, 이어서 파서의 줄 끝 주석을 처리한다. 문서를 따른 사용자의 설정이 조용히 뒤집힌다.
3. **ISSUE-003:** 관리자 로그인 작업의 경로 검사·경고와 관리자 인스턴스 업데이트 정책. 이 기능이 만든 영속 승격 경로다.

---

## 10. 적용 기록 (2026-10-22)

이 감사 직후 사용자 요청으로 1~8장의 제안을 모두 구현했다. 게이트(`fmt`·`clippy -D warnings`·`check --workspace`)와 `cargo test --workspace`가 통과했다(221건, 앱 단위 테스트 178 → 190).

| 제안 | 적용 내용 | 테스트 |
|---|---|---|
| ISSUE-001 | `storage::strip_inline_comment`: "공백 + `#`/`;`" 이후는 주석, 값 전체가 주석이면 빈 값. `lpSoundFile`은 그대로, `sAutoDisableApps`의 `;`는 구분자로 유지. README 샘플은 그대로 두고 주석 규칙만 문서화 | `readme_sample_parses_to_what_it_documents`(README 두 샘플을 `include_str!`로 파싱), `trailing_comments_need_whitespace_and_keep_paths_and_app_lists` |
| ISSUE-002 | `write_durably`(tmp `sync_all`), `replace_durably`(`MoveFileExW` + `WRITE_THROUGH`), `looks_like_settings`(헤더·알려진 키가 없으면 손상으로 보고 `.bak` 폴백), 유효한 본 파일만 `.bak`으로 복사 | `unrecognizable_main_file_falls_back_to_the_backup`, `save_never_backs_up_a_torn_main_file` |
| ISSUE-003 (a) | `exe_in_protected_location`(Program Files·Windows known folder 하위 판정). 밖이면 작업 켜기 전 경고(기본 No) | `protected_location_means_inside_a_protected_root`, `known_protected_roots_resolve` |
| ISSUE-003 (b) | `update_route`: 관리자 실행 중이면 인앱 업데이트 대신 안내(패키지 관리자 안내가 우선) | `elevated_copies_never_update_in_app` |
| ISSUE-004 | `watchdog::check`에 `elevated_foreground` 인자 추가, `foreground::status().1`이 참이면 프로브 생략 | `no_probe_while_an_administrator_window_is_in_front` |
| ISSUE-005 | `on_save` 기준 = `save_base(try_load(), SAVED)` | `save_starts_from_the_file_but_never_from_an_unreadable_one` |
| 5장 표식 Gap | 대기 시간 초과 시 표식 삭제. 늦게 뜬 관리자 작업이 조용히 종료할 때 표식 재설정 | 관리자·로그인 경로라 수동 체크리스트로 (`scripts/e2e/README.md`) |
| 5장 재주입 Gap | injector 경로 `send_retrying`(100ms × 최대 30회, 남은 것만 재전송). 종료 경로는 1회 | `refused_releases_are_retried_from_where_they_stopped` |
| 5장 자동 실행 창 | `--autostart`이면 항상 트레이로 시작 | GUI 경로(테스트 금지 규칙) |
| 5장 비UTF-8 | `read_text`: BOM 제거, UTF-8이 아니면 시스템 코드 페이지(`MultiByteToWideChar(CP_ACP)`)로 해석 | `non_utf8_file_is_read_in_the_system_code_page` |
| 5장 `--after-pid` | 대기 10 → 30초 | 상수 변경 |
| 5장 관리자 E2E 부재 | `scripts/e2e/README.md`에 관리자 경로 수동 체크리스트 6항목 추가 | 수동 |
| 5장 프록시 | `HTTPS_PROXY` 등이 없으면 `WinHttpGetIEProxyConfigForCurrentUser`의 수동 프록시 사용(PAC 미지원) | `windows_proxy_setting_picks_the_https_route`. 실제 `--check-update` exit 0 확인 |
| 6장 문서 | CLAUDE.md(쓰기 경로 목록·저장 규칙·워치독·업데이트 경로·프록시·재주입), `docs/UPDATE.md`(`--autostart`·`--after-pid`·관리자 업데이트·프록시), README(주석 규칙·트레이 시작·관리자 작업 위험·프록시, 영/한), `staging.rs` 주석, `AGENTS.md` 신설(CLAUDE.md 안내), CHANGELOG | 문서 |

미실행: 관리자·UAC 실기 체크리스트와 GUI E2E 스크립트. 이 세션은 비관리자이고, 프로젝트 규칙상 테스트에서 GUI를 띄우지 않는다.

*적용 기록 끝. 1~9장의 판단은 감사 시점 기준이다.*