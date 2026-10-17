# Project Audit

- 대상: `keyboard-chattering-fix` (`kcfix-rs`) — 메인 바이너리 + `crates/kcfix-updater` 헬퍼 (Cargo 워크스페이스)
- 기준 시점: 2026-10-17, `main` 브랜치 작업 트리(커밋 `a280cad` + 미커밋 UI 분할 변경분 포함)
- 방법: 문서 확인 → CodeGraph 탐색 → 소스 직접 열람 → 테스트 실행. **코드는 수정하지 않았다.**
- 이 문서는 이전 `PROJECT_AUDIT.md`(같은 날 작성, `a280cad`에 커밋됨)를 대체한다. 이전 문서의
  ISSUE-001(롤백 재시도)·002(GDI 브러시 캐시)·004(저장 실패 알림)는 코드에서 해소된 것을 다시 확인했다.

---

## 1. Executive Summary

**전체 상태.** 필터 핵심(`Debouncer::decide`), 설정 직렬화, 업데이트 매니페스트 검증(Ed25519 + 핀 고정 URL + 이중 SHA-256)은
정확하고 테스트도 촘촘하다(Rust 110개, Python 7개 모두 통과). 문제는 주로 **프로세스 수명주기와 업데이트 흐름**에 있다.
한 번 동작하고 나면 망가지거나(업데이트 헬퍼), 정상 종료가 아닌 경로(OS 종료, UI 정지)에서 상태가 사라지는 유형이다.

**전체 위험도: Medium.** 데이터를 파괴하는 경로는 없다. 다만 자동 업데이트가 두 번째부터 동작하지 않는 문제(확정)와,
필터가 아무 표시 없이 꺼질 수 있는 구조적 문제(유력)가 있다.

**가장 중요한 문제**

1. **[ISSUE-001] 자동 업데이트가 첫 번째 이후 영구적으로 실패함** — 헬퍼(`kcfix-updater.exe`)는 교체되지 않는데, 엄격한 버전 스탬프 검사가 새 앱 버전과 헬퍼 버전의 일치를 요구한다. (High / Confirmed)
2. **[ISSUE-002] 저수준 훅이 UI 스레드에서 실행됨** — UI가 약 1초 이상 멈추면 Windows가 훅을 아무 알림 없이 제거한다. 이후 필터는 꺼진 상태지만 UI는 "보호 중"으로 표시된다. (High / Likely)
3. **[ISSUE-003] 업데이트 "적용 중" 대화상자를 30초 안에 닫지 않으면** 헬퍼가 대기 시간 초과로 포기한다. 앱은 그 뒤에 종료되므로 필터가 실행되지 않는 상태로 남는다. (Medium / Confirmed)
4. **[ISSUE-004] Windows 종료·로그오프 시 통계와 보호 상태를 저장하지 않음** (`WM_ENDSESSION` 미처리). (Medium / Confirmed)
5. **[ISSUE-005] 실행 중에 `settings.ini`를 직접 수정하면** 종료·언어 변경·통계 초기화 시점에 기동 시점의 스냅샷으로 덮어쓰여 사라진다. 키별 임계값(`key.XX`)은 파일로만 설정할 수 있어서 영향이 직접적이다. (Medium / Confirmed)

**데이터 손상/유실 가능성.** 설정 파일 손상 경로는 없다(tmp → rename 원자 저장, `.bak` 폴백). 현실적인 유실은
(a) OS 종료 시 세션 통계·보호 토글 미저장(ISSUE-004),
(b) 실행 중 외부에서 편집한 설정의 덮어쓰기(ISSUE-005)다. 둘 다 사용자 설정과 통계 수준이고, 바이너리나 시스템 손상은 없다.

**가장 먼저 수정할 영역.** `src/updater/staging.rs` + `crates/kcfix-updater`(헬퍼 갱신 전략) →
`src/app/bootstrap.rs`·`src/input/hook.rs`(훅 전용 스레드 또는 훅 생존 감시) → `src/ui/window.rs`(`WM_ENDSESSION` 처리).

---

## 2. Project Understanding

### 목적
Windows `WH_KEYBOARD_LL` 저수준 키보드 훅으로 기계식 스위치 바운스(채터링)를 가로채 버리는 상주형 필터다.
관리자 권한이 필요 없고, Win32 네이티브 UI(탭·트레이)를 쓴다. 청소 모드, 실시간 통계, Ed25519 서명 기반 수동 업데이트를 제공한다.
설계상 Windows 전용이다.

### 주요 entrypoint
| Entrypoint | 위치 | 역할 |
|---|---|---|
| `main` | `src/main.rs:28` | `app::cli::handle_cli()`(`--version`/`-V`/`--check-update`) → `app::bootstrap::run(config::load())` |
| `bootstrap::run` | `src/app/bootstrap.rs:78` | `init_shared` → 단일 인스턴스 뮤텍스 → 업데이트 핸드셰이크 ack → `timeBeginPeriod(1)` → `create_window` → `install_hook` → 메시지 루프 |
| `hook_proc` | `src/input/hook.rs:17` | OS가 키 이벤트마다 호출하는 필터 콜백(**설치한 스레드 = UI 스레드에서 실행**) |
| `wndproc` | `src/ui/window.rs:316` | 메시지 라우팅(타이머·명령·트레이·종료) |
| 헬퍼 `main` | `crates/kcfix-updater/src/main.rs` | 대기 → 해시 재검증 → 교체 → 재실행·준비 확인 → 실패 시 롤백·이전 버전 재실행 |

### 핵심 모듈
| 모듈 | 역할 |
|---|---|
| `debounce.rs` | `Debouncer::decide` 상태 머신(키별 임계값, release/press 기준, 오토리핏 보존, 청소 모드, 로그 링버퍼) |
| `app/state.rs` | `SHARED: OnceLock<Mutex<Shared>>`(훅·UI가 공유), `START`, `HOOK` |
| `config/` | `Settings` 모델, INI 원자 저장(`storage`), Run 레지스트리(`autostart`), 사운드 경로 사전 검증(`sound`) |
| `ui/` | `window`(디스패치), `settings_form`(저장·적용), `stats`/`history`(스냅샷 → 렌더 → diff), `commands`, `tray`, `update`(워커 스레드) |
| `updater/` | 매니페스트 서명·형식·만료 검증, 다운로드·해시, 헬퍼 해석·스테이징·실행 |

### 데이터 저장
- `%APPDATA%\KeyboardChatteringFix\settings.ini`(레거시 `KcfixRs` 폴백). 저장 순서는 `settings.ini.tmp` 기록 → 기존 파일을 `.bak`으로 복사 → rename.
- 저장 시점은 다음 넷뿐이다: Save 버튼(`on_save`), 언어 변경, 통계 초기화, `WM_DESTROY`(`persist_live`).
- `HKCU\...\Run\KeyboardChatteringFix`(따옴표로 감싼 경로).
- DB와 서버 상태는 없다.

### 외부 의존성
`windows 0.62`, `ureq 3`(HTTP, timeout 30/60/90s), `ed25519-dalek 3`, `sha2`, `serde_json`, `tracing`. 헬퍼는 `clap`을 쓴다.
GitHub Releases(`releases/latest/download/update.json`)에 의존한다.

### 핵심 실행 흐름
- **키 입력:** `OS 키 이벤트 → hook_proc(UI 스레드) → SHARED.lock → Debouncer::decide → LRESULT(1)(차단) / CallNextHookEx(통과) → (차단 + 사운드) PlaySoundW 비동기`
- **설정 저장:** `Save/Enter → on_save → 폼 파싱·클램프 → set_autostart → config::load()(디스크 재읽기) → resolve_sound_file(락 밖) → SHARED 갱신 → config::save → SAVED 갱신 → 상태 표시/실패 대화상자`
- **통계 표시:** `WM_TIMER(500ms) → update_stats: 스냅샷(짧은 락) → render_stats → diff_stats → 바뀐 컨트롤만 SetWindowTextW`
- **업데이트:** `메뉴/트레이 → spawn_update_check(워커) → check_for_update(서명·URL·만료·버전) → Yes → prepare_update(resolve_helper → %TEMP% 스테이징 → 다운로드·해시) → launch_helper → "적용 중" MessageBox → WM_UPDATE_EXIT → DestroyWindow → 헬퍼: pid 대기(30s) → 해시 재검증 → rename 교체 → 재실행·준비 확인(60s) → 백업 삭제 / 실패 시 롤백`
- **종료:** `WM_CLOSE(close_to_tray면 숨김) / MENU_EXIT / WM_UPDATE_EXIT → WM_DESTROY → persist_live → 트레이 제거 → UnhookWindowsHookEx → PostQuitMessage`

---

## 3. Audit Coverage & Limitations

### 직접 확인한 모듈(전체 열람)
`main.rs`, `app/{bootstrap,cli,instance,state}.rs`, `input/{hook,sound}.rs`, `debounce.rs`(본체),
`config/{model,storage,sound,autostart}.rs`, `config.rs`, `ui/{window,settings_form,commands,state,update,dialog}.rs`,
`ui/{stats,history,tray}.rs`(핵심 함수), `ui/text.rs`(일부), `i18n/language.rs`,
`updater/{http,staging,model,version}.rs`, `updater/manifest.rs`(검증 본체),
`crates/kcfix-updater/{src/lib.rs,src/main.rs,build.rs,Cargo.toml}`,
`.github/workflows/*`·`scripts/build_release.ps1`·`docs/UPDATE.md`(헬퍼 배포 관련 부분만 grep).

### CodeGraph로 분석한 호출 관계
- `update_stats` ← `wndproc(WM_TIMER)`, `commands::set_protection/set_cleaning/run`, `settings_form::on_save`(7개 caller)
- `history::refresh` ← `apply_visibility`, `WM_TIMER`, `commands::run`, `on_view_changed`/`on_search_changed`
- `install_hook` ← `bootstrap::run`(유일), `hook_proc` ← `install_hook`(유일)
- `resolve_helper` ← `prepare_update` ← `ui::update::spawn_update_check`, `launch_helper` ← `spawn_update_check`/`apply_update`(dead code, `#[allow(dead_code)]`)
- `persist_live` ← `window::on_destroy`, `commands::run(MENU_RESET_STATS)`, `on_language_changed`(grep으로 보완)
- `config::save` ← `persist_live`, `on_save`
- CodeGraph는 `explore` 2회와 프롬프트 훅 컨텍스트로 사용했다. 정확한 caller 목록이 필요한 곳(`persist_live`, `WM_ENDSESSION` 부재, `exe.old`, `timeout_secs`)은 grep으로 보완했다.

### 실행한 테스트
| 명령 | 결과 |
|---|---|
| `cargo test --workspace` | **110 passed, 0 failed**(메인 90 + 헬퍼 lib 2 + 헬퍼 통합 10 + exe_smoke 2 + version_sync 6) |
| `python -m pytest scripts/ -q` | **7 passed** |
| `dist/*.exe` VersionInfo 조회 | 앱·헬퍼 모두 `ProductVersion 0.2.0.0`(ISSUE-001 근거) |

실행 시점에 앱이 떠 있지 않아 `second_acquisition_fails_while_first_held`도 통과했다.

### 확인하지 못한 것
- GUI 실제 구동, 실제 키 입력 기반 훅 동작(CLAUDE.md 규칙상 GUI 테스트는 하지 않았다).
- 실제 GitHub Release를 대상으로 한 업데이트 E2E(0.2.0 → 0.3.0 → 0.4.0 연속 업데이트), 네트워크 실패 재현.
- `LowLevelHooksTimeout` 초과 시 훅이 제거되는 현상의 실측(ISSUE-002는 Win32 문서상 동작과 코드 구조에 근거한 Likely 판정).
- `cargo clippy`/`cargo fmt --check`는 감사 범위상 실행하지 않았다.
- `ui/layout.rs`, `ui/widgets.rs`, `ui/paint.rs`, `ui/menu.rs`, `ui/keys.rs`는 기능 위험이 낮다고 보고 테스트 존재 여부만 확인했다(정밀 열람 안 함).

---

## 4. High-Risk Issues

### [ISSUE-001] 인앱 업데이트가 첫 번째 이후 영구적으로 실패함(헬퍼 미갱신 + 엄격한 버전 스탬프 검사)

- **위치:** `src/updater/staging.rs` `resolve_helper`(120–155), `helper_stamp_compatible`(99–118); `crates/kcfix-updater/src/lib.rs` `update_executable_with`(182–293); `crates/kcfix-updater/build.rs`(17–36)
- **우선순위:** High
- **신뢰도:** Confirmed(코드 흐름 + 배포 바이너리 스탬프 확인)
- **문제:** 업데이트는 `--current`(메인 exe)만 교체하고, 앱 옆의 `kcfix-updater.exe`는 손대지 않는다. 그런데 `resolve_helper`는 앱 옆 헬퍼를 "authoritative"로 취급하면서, 헬퍼의 ProductVersion이 **현재 실행 중인 앱의 `VERSION`**(또는 `VERSION.0`)과 같지 않으면 오류로 중단한다. 헬퍼 스탬프는 빌드 시 워크스페이스 버전(`0.2.0.0`)으로 고정된다.
- **발생 조건:** 0.2.0 → 0.3.0 업데이트는 성공한다(0.2.0 앱 + 0.2.0 헬퍼). 이후 0.3.0 앱에서 다음 업데이트를 시도하면 옆의 헬퍼는 여전히 `0.2.0.0`이므로 `"kcfix-updater.exe 헬퍼의 버전이 앱과 일치하지 않습니다"` 오류가 난다.
- **영향:** 자동 업데이트 기능이 1회용이 된다. 사용자가 zip을 받아 수동으로 헬퍼를 교체하기 전까지 모든 후속 업데이트가 실패한다.
- **근거:**
  - `launch_helper`가 넘기는 인자는 `--current <app exe>`, `--replacement <staged app>`뿐이다(staging.rs:203–215).
  - 헬퍼는 `current`만 rename/교체한다(lib.rs:226–254). 헬퍼 자신을 갱신하는 코드는 없다.
  - `helper_stamp_compatible(Some("0.2.0.0"))`는 `VERSION = "0.3.0"`일 때 false를 반환한다(staging.rs:109–114, 테스트 `helper_stamp_compatible_accepts_legacy_and_matching`가 `"0.0.0"` 거부를 확인).
  - `dist/kcfix-updater.exe` ProductVersion = `0.2.0.0`(실측).
- **반증 확인:**
  - 매니페스트나 릴리스가 헬퍼를 함께 배포하는가? `release.yml`은 `kcfix-updater.exe`와 zip을 릴리스 자산으로 올린다. 하지만 앱 업데이트 흐름은 `update.json`의 `artifact_url`(메인 exe 하나, `is_valid_artifact_url`로 파일명 고정)만 받는다. 헬퍼 자산을 받는 경로는 없다.
  - 폴백이 있는가? 앱 옆에 헬퍼가 **존재**하면 dev-tree 폴백으로 넘어가지 않고 즉시 Err를 반환한다(staging.rs:125–137).
  - 스탬프가 없는 경우(None)만 legacy로 허용하는데, 현재 빌드는 항상 스탬프가 있다.
  - 결론: 보호 장치가 없다.
- **호출/영향 범위:** `ui::update::spawn_update_check` → `prepare_update` → `resolve_helper`. CLI `--check-update`는 확인만 하므로 영향이 없다. 헬퍼의 롤백·재실행 로직은 정상이다.
- **권장 수정 방향(택1 또는 조합):**
  1. 헬퍼 호환성을 "앱 버전과 동일"이 아니라 **프로토콜 버전**(예: 헬퍼 CLI 계약 버전 상수)으로 판정한다. 또는 "헬퍼 ≤ 앱이면 허용"처럼 완화한다.
  2. 새 앱 exe가 첫 실행 시(핸드셰이크 후) 자기 리소스에 내장한 헬퍼를 꺼내 옆 헬퍼를 갱신한다.
  3. 매니페스트에 헬퍼 해시·URL을 서명 대상으로 포함하고 함께 교체한다.
- **필요한 회귀 테스트:** `helper_stamp_compatible`에 "이전 릴리스 헬퍼 스탬프(예: `0.2.0.0`)를 다음 마이너 앱 버전에서 수락" 케이스를 추가한다. `resolve_helper`에는 스탬프된 구버전 헬퍼가 옆에 있을 때 Ok를 반환하는 통합 테스트를 둔다(테스트 빌드에서 `VERSION`을 주입할 수 있게 함수를 순수화). 릴리스 체크리스트에는 "N → N+1 → N+2 연속 업데이트 E2E"를 넣는다.

---

### [ISSUE-002] 저수준 키보드 훅이 UI 스레드에서 실행됨 — UI가 멈추면 훅이 아무 알림 없이 제거되고 UI는 계속 "보호 중"으로 표시함

- **위치:** `src/app/bootstrap.rs` `run`(113–126), `src/input/hook.rs` `install_hook`/`hook_proc`, UI 스레드에서 동기 작업을 하는 곳들(`settings_form::on_save`, `persist_live`, `commands::shell_open` 등)
- **우선순위:** High
- **신뢰도:** Likely(코드 구조는 확정, 훅 제거는 Windows 문서상 동작이며 실측은 못 함)
- **문제:** `WH_KEYBOARD_LL` 콜백은 **훅을 설치한 스레드의 메시지 루프**에서 호출된다. 이 앱은 메인 UI 스레드에서 훅을 설치하고, 같은 스레드에서 창 메시지를 처리한다. Windows 7 이후로는 콜백이 `LowLevelHooksTimeout`(Win10 1709+ 최대 1000ms) 안에 돌아오지 않으면 훅을 **알림 없이 제거**한다(MS 문서: "There is no way for the application to know whether the hook is removed").
- **발생 조건:** UI 스레드가 키 입력 도중 약 1초 이상 블록되는 경우다. 후보는 다음과 같다.
  - `ShellExecuteW`(설정 폴더 열기, 홈페이지 열기 — 브라우저나 셸 콜드 스타트 시 수 초 걸릴 수 있음)
  - `on_save` 안의 레지스트리·디스크 I/O
  - 사운드 경로가 응답 없는 네트워크 경로일 때의 `resolve_sound_file`(`is_file()`)
  - CLAUDE.md에 기록된 "GUI 기동 직후 응답없음" 현상 자체
- **영향:**
  1. UI가 멈춘 동안 **시스템 전체 키 입력이 지연**된다(모든 키 이벤트가 이 스레드를 거침).
  2. 타임아웃 뒤 훅이 제거되면 채터링 필터가 꺼진다. 그런데 `HOOK` 핸들과 `enabled` 플래그는 그대로라서 트레이와 상태 알약은 "보호 중"을 계속 표시한다. 재시작 전까지 복구되지 않는다.
- **근거:**
  - `install_hook`은 `bootstrap::run`(메인 스레드)에서 호출되고, 그 스레드가 곧바로 `GetMessageW` 루프를 돈다(bootstrap.rs:113–126).
  - 훅 생존을 확인하거나 재설치하는 코드가 없다(`HOOK`은 `WM_DESTROY`에서만 읽음, window.rs:309).
  - `hook.rs` 모듈 주석은 "Runs on the system hook thread"라고 쓰고 있어 실제 모델과 다르다.
- **반증 확인:**
  - 모달 루프(`MessageBoxW`, `TrackPopupMenu`, `GetOpenFileNameW`)는 메시지를 펌프하므로 훅이 계속 서비스된다. 이 경로들은 문제가 아니다.
  - `update_stats`/`history::refresh`는 락을 짧게 잡고 Win32 호출 전에 놓는다. 같은 스레드라 훅과 락 경합은 없다.
  - 네트워크 업데이트는 워커 스레드에서 돈다.
  - 그래도 펌프하지 않는 동기 호출(셸 실행, I/O)과 원인 미상의 응답없음은 남는다. 정상 경로에서는 대부분 짧기 때문에 Likely로 둔다.
- **호출/영향 범위:** `hook_proc`(모든 키 입력), `window::wndproc`의 모든 동기 핸들러, `stats::update_stats`의 상태 표시(잘못된 "보호 중").
- **권장 수정 방향:** 훅 설치와 메시지 펌프를 **전용 훅 스레드**로 분리한다. 그 스레드는 `GetMessageW` 루프만 돌고 UI 작업은 하지 않는다. `SHARED` 락은 이미 스레드 간 공유를 전제로 설계되어 있다. 보조 조치로 `ShellExecuteW`를 워커 스레드로 옮기고, 사운드 경로 검증을 비동기로 바꾼다.
- **필요한 회귀 테스트:** (단위) 훅 스레드 생성 함수가 스레드 ID를 반환하고 UI 스레드 ID와 다른지 확인. (수동/플랫폼) 디버그 빌드에 UI 스레드 2초 sleep 메뉴를 숨겨 두고 실행해, 이후에도 채터링 차단 카운터가 증가하는지 확인한다(실제 키보드 사용, `APPDATA` 임시 폴더).

---

### [ISSUE-003] 업데이트 "적용 중" 대화상자가 헬퍼의 30초 대기 시간을 소모함 — 늦게 닫으면 업데이트는 실패하고 앱은 꺼진 채로 남음

- **위치:** `src/ui/update.rs` `spawn_update_check`(284–310); `src/updater/staging.rs` `launch_helper`(201–226); `crates/kcfix-updater/src/main.rs`(`timeout_secs` 기본 30), `lib.rs` `should_relaunch_previous`
- **우선순위:** Medium
- **신뢰도:** Confirmed(코드 흐름)
- **문제:** `launch_helper`로 헬퍼를 **먼저 실행**한 뒤 `msg_box(update_applying, MB_OK)`로 사용자 확인을 기다리고, 확인한 다음에야 `WM_UPDATE_EXIT`를 보낸다. 헬퍼는 `--timeout-secs`를 받지 않으므로 기본 30초만 앱 종료를 기다린다. 대기 시간을 넘기면 `WaitTimeout`이 되고, 이 경우 `should_relaunch_previous`가 false라서 이전 버전도 다시 띄우지 않는다.
- **발생 조건:** 사용자가 "업데이트를 적용합니다" 대화상자를 30초 넘게 두는 경우다. 예를 들어 트레이에서 업데이트를 시작해 소유 창이 숨겨진 상태에서 대화상자가 다른 창 뒤에 깔린 경우.
- **영향:** 헬퍼가 "상위 프로세스 대기 시간 초과" 오류를 띄우고 스테이징 파일을 삭제한다. 그 뒤 사용자가 OK를 누르면 앱이 종료된다. 결과적으로 **업데이트는 적용되지 않았고 필터도 실행되지 않는** 상태가 된다. 데이터 손상은 없고, 수동으로 다시 실행하면 복구된다.
- **근거:** update.rs:286(`launch_helper`) → 302–309(`msg_box` 후 `PostMessageW(WM_UPDATE_EXIT)`). staging.rs:204–215에 `--timeout-secs` 인자가 없다. 헬퍼 main.rs:95–96 `default_value_t = 30`, lib.rs:102–104.
- **반증 확인:** 헬퍼의 대기는 프로세스 종료를 기다리는 것이라 대화상자와 무관하게 시계가 흐른다. 대화상자가 자동으로 닫히는 장치는 없다(MB_OK 모달). 보호 장치가 없다.
- **호출/영향 범위:** 수동 업데이트 흐름 전체(메뉴 바·트레이 공용 `MENU_UPDATE`).
- **권장 수정 방향:** "적용 중" 안내를 헬퍼 실행 **이전**의 Yes/No 확인에 합치고, 헬퍼 실행 직후 바로 `WM_UPDATE_EXIT`를 보낸다. 또는 `--timeout-secs`를 충분히 크게 넘기고, 시간 초과 시에도 앱이 이미 종료됐다면 이전 버전을 재실행한다(`WaitTimeout` 뒤 pid 생존 재확인).
- **필요한 회귀 테스트:** (헬퍼 통합) 죽지 않는 더미 pid로 `timeout=1s`를 주면 `WaitTimeout`이 나는지 확인(기존). 여기에 "대기 초과 후 pid가 사라졌으면 이전 버전 재실행" 같은 새 정책을 단위 테스트한다. (앱) `launch_helper`가 받는 인자 목록 스냅샷 테스트로 `--timeout-secs` 포함 여부를 고정한다.

---

### [ISSUE-004] Windows 종료·로그오프 시 통계와 보호 상태가 저장되지 않음

- **위치:** `src/ui/window.rs` `wndproc`(316–411), `on_destroy`(305–314); `src/ui/settings_form.rs` `persist_live`(164–178)
- **우선순위:** Medium
- **신뢰도:** Confirmed(코드 흐름 + Win32 수명주기)
- **문제:** 누적 통계(`total/blocked`, 키별 `stat.XX`)와 즉시 적용되는 보호 토글은 `persist_live`에서만 디스크에 쓰인다. 이 함수는 `WM_DESTROY`, 통계 초기화, 언어 변경에서만 호출된다. 앱은 `WM_QUERYENDSESSION`/`WM_ENDSESSION`을 처리하지 않는다(소스 전체 grep 0건). Windows는 세션 종료 시 `WM_ENDSESSION` 뒤에 `WM_DESTROY` 없이 프로세스를 종료할 수 있다.
- **발생 조건:** 트레이에 상주한 채로 PC를 종료하거나 로그오프하거나 재시작하는 경우다. 상주형 앱이라 이것이 **가장 흔한 종료 경로**다.
- **영향:** 마지막 Save 이후 세션 동안 쌓인 통계가 사라진다. 그래서 "재시작 후에도 유지되는 최다 불량 키 진단"(`model.rs` 주석, README)이 일반적인 사용 패턴에서는 사실상 동작하지 않는다. 보호 토글을 꺼 둔 상태도 재부팅하면 켜짐으로 돌아간다(안전한 방향이긴 하다).
- **근거:** `persist_live` caller는 `on_destroy`, `MENU_RESET_STATS`, `on_language_changed`뿐이다. 주기 저장 타이머는 없다(`SetTimer`는 `TIMER_STATS` 하나, 저장은 하지 않음).
- **반증 확인:** `DefWindowProcW`는 `WM_ENDSESSION`에서 아무것도 저장하지 않는다. Rust 런타임에도 종료 훅이 없다. `close_to_tray`와 무관하다. 보호 장치가 없다.
- **호출/영향 범위:** `config::save`(`persist_live` 경로), 홈 탭 통계·기록 탭 키별 통계의 재시작 후 표시.
- **권장 수정 방향:** `WM_ENDSESSION`(wParam=TRUE)에서 `persist_live()`를 호출하고 트레이를 제거한다. 여기에 저빈도 주기 저장(예: 변경이 있을 때만 5분마다)이나 차단 수 증가분 기준 저장을 더한다.
- **필요한 회귀 테스트:** (단위) `wndproc`에서 메시지 분기를 순수 함수로 분리해 `WM_ENDSESSION(TRUE)`가 "persist" 액션을 반환하는지 검증한다. (수동) `APPDATA` 임시 폴더로 실행해 몇 키를 입력하고 `shutdown /l`로 로그오프한 뒤, 재로그인했을 때 `iTotalPresses`가 0보다 큰지 확인한다.

---

### [ISSUE-005] 실행 중 `settings.ini`를 직접 편집하면 종료·언어 변경·통계 초기화 때 기동 시점 스냅샷으로 덮어써짐

- **위치:** `src/ui/settings_form.rs` `persist_live`(164–178), `on_language_changed`(181–193); `src/ui/window.rs` `on_create`(163, `SAVED` 초기화); `src/ui/commands.rs` `MENU_OPEN_CONFIG`
- **우선순위:** Medium
- **신뢰도:** Confirmed(코드 흐름)
- **문제:** `persist_live`는 메모리의 `SAVED`(기동 시 로드한 설정 + 이후 Save 결과)에 라이브 카운터만 덮어 그대로 저장한다. 디스크를 다시 읽지 않는다. 반면 `on_save`는 `config::load()`로 디스크를 다시 읽어 병합한다. 두 경로의 동작이 서로 다르다.
- **발생 조건:** 키별 임계값(`key.XX`)은 **UI가 없고 설정 파일로만** 지정할 수 있다(CLAUDE.md "알려진 주의점", README). 앱에는 "설정 폴더 열기" 메뉴도 있다. 앱을 켠 채로 `key.41=120`을 추가하고 저장한 다음, 적용하려고 트레이 → 종료(또는 언어 변경, 통계 초기화)를 하면 `persist_live`가 `key.41`이 없는 스냅샷으로 파일을 덮어쓴다.
- **영향:** 사용자가 직접 넣은 키별 임계값과 그 밖의 수동 편집이 조용히 사라진다. 재시작해도 적용되지 않는다. 사용자가 "앱을 먼저 종료한 뒤 편집"하면 피할 수 있다(문서화되어 있지 않음).
- **근거:** `persist_live`에는 `config::load` 호출이 없다(168–177). `SAVED`는 `on_create`에서 `init.clone()`으로 한 번 설정되고, 이후 `on_save`(308)와 언어 변경(188)에서만 바뀐다.
- **반증 확인:** Save 버튼을 한 번 누르면 `on_save`가 디스크를 다시 읽으므로 그 뒤의 `persist_live`는 안전하다. 하지만 "파일 편집 → 종료" 순서에는 이 보호가 작동하지 않는다. `.bak`은 바로 직전 세대만 보관하므로, 덮어쓰기 직후의 `.bak`에 편집본이 남긴 한다. 다만 load는 메인 파일이 비어 있을 때만 `.bak`을 읽으므로 자동으로 복구되지 않는다.
- **호출/영향 범위:** `persist_live` ← `on_destroy`(모든 정상 종료, 업데이트 종료 포함), `MENU_RESET_STATS`, `on_language_changed`.
- **권장 수정 방향:** `persist_live`도 `config::load()`를 기준으로 삼고, 앱이 소유한 필드(카운터·enabled·language)만 덮어쓰게 해 `on_save`와 같은 병합 의미로 통일한다. 또는 파일 mtime이 바뀐 경우에만 다시 읽는다. 아울러 `on_save`에서 `per_key_ms`를 디바운서에 즉시 적용하면 "편집 → Save로 반영"이라는 안전한 워크플로가 생긴다.
- **필요한 회귀 테스트:** (단위, `with_isolated_appdata` 재사용) 기동 스냅샷 A를 저장하고, 파일에 `key.41=120`을 추가한 뒤 `persist_live` 등가 함수(순수화 필요)를 호출하면 `load().per_key_ms[0x41] == Some(120)`이어야 한다.

---

### [ISSUE-006] 업데이트 오류 메시지가 UI 언어와 무관하게 한국어로만 표시됨

- **위치:** `src/updater/{http,manifest,staging}.rs`의 `UpdateError::Message("...")` 문자열, `src/ui/update.rs`(`err.to_string()`을 그대로 표시), `crates/kcfix-updater/src/lib.rs` `HelperError`, 헬퍼 `main.rs` 대화상자 제목 `"업데이트 실패"`
- **우선순위:** Low
- **신뢰도:** Confirmed
- **문제:** 영어 UI 사용자에게도 다운로드 실패, 서명 실패, 헬퍼 오류 등이 한국어로 나온다. CLAUDE.md는 "모든 가시 문자열은 한/영 쌍으로 존재"를 규칙으로 명시하고 있다.
- **영향:** 영어 사용자는 실패 원인을 알 수 없다. 기능 자체에는 영향이 없다.
- **반증 확인:** `classify_update_result`는 메시지를 가공하지 않고 그대로 전달한다(테스트 `classify_preserves_error_message`). 번역 계층이 없다.
- **권장 수정 방향:** `UpdateError`를 의미 코드 enum(`DownloadFailed`, `SignatureInvalid`, `HelperVersionMismatch` …)으로 바꾸고 UI에서 `strings()`로 매핑한다. 헬퍼에는 `--lang`을 전달한다.
- **필요한 회귀 테스트:** 각 `UpdateError` 변형마다 Ko/En 문자열이 모두 비어 있지 않은지 검사한다(기존 i18n 테스트 패턴 확장).

---

## 5. Potential Functional Gaps

| # | 분류 | 내용 |
|---|---|---|
| G1 | **Confirmed Gap** | **훅 설치 실패를 무시한다.** `install_hook`이 `unwrap_or_default()`로 null HHOOK을 반환해도 그대로 진행한다. 이때 UI는 "보호 중"으로 표시되지만 필터는 동작하지 않는다(bootstrap.rs:113–114). 실패 가능성은 낮지만 알림이나 재시도가 없다. |
| G2 | **Confirmed Gap** | **자동 시작 레지스트리 쓰기 결과를 무시한다.** `on_save`에서 `config::set_autostart(autostart)`의 bool을 버린다(settings_form.rs:271). 실패해도 토글은 켜진 것처럼 보이고, 다음 기동에서야 실제 상태가 드러난다. |
| G3 | **Confirmed Gap** | **키별 임계값이 Save로 적용되지 않는다.** `on_save`는 디스크의 `per_key_ms`를 다시 저장만 하고 디바운서에는 적용하지 않는다(`set_key_threshold` 호출은 `init_shared`에만 있음). 재시작해야 반영된다. ISSUE-005와 결합된다. |
| G4 | **Confirmed Gap** | **스테이징한 헬퍼 사본이 남는다.** 성공적인 업데이트 뒤에도 `%TEMP%\kcfix-updater_helper_*.exe`가 남는다. 헬퍼는 자신을 지울 수 없고, 앱도 기동 시 정리하지 않는다. 업데이트마다 한 개(수백 KB)씩 쌓인다. |
| G5 | Likely Gap | **훅 생존 감시가 없다.** ISSUE-002와 별개로 다른 원인(세션 전환, 보안 소프트웨어)으로 훅이 제거되어도 알 수 없다. 기록 탭의 "마지막 이벤트 시각" 같은 지표나 주기적 재설치 옵션이 없다. |
| G6 | 추정 | `bExcludeInjected=0`일 때 `VK_PACKET`(0xE7) 유니코드 주입 입력(화상 키보드, 비밀번호 관리자 자동 입력)은 모든 문자가 같은 vk를 공유하므로, 임계값 안에서 연속으로 들어오면 문자가 차단될 수 있다. 기본값(1)에서는 해당 없다. |
| G7 | 추정 | 사운드 경로가 응답 없는 UNC 경로면 `resolve_sound_file`(`is_file`)이 UI 스레드를 수 초 막을 수 있고, 이것이 ISSUE-002를 유발한다. |
| G8 | Confirmed(무해) | 헬퍼가 넘기는 `--minimized` 재실행 인자를 앱 CLI가 해석하지 않는다(`cli.rs`는 `--version`/`--check-update`만 처리). 현재는 넘기는 쪽도 없어 실제 영향은 없다. |
| G9 | Likely Gap | `get_text` 버퍼가 1024 UTF-16 단위라 그보다 긴 사운드 경로는 잘린 채 저장된다. 긴 경로(`\\?\`)를 쓰는 경우에만 해당한다. |

---

## 6. Documentation Mismatches

| 문서 | 기술 | 실제 |
|---|---|---|
| `README.md` (EN 138행, KO 277행) | "79 Rust tests" / "79개 Rust 테스트" | `cargo test --workspace` 결과 **110개**(메인 90 + 헬퍼 12 + 통합 8) |
| `src/input/hook.rs` 모듈 주석 | "Runs on the system hook thread" | LL 훅은 **설치한 스레드(UI 스레드)** 의 메시지 루프에서 실행됨(ISSUE-002) |
| `CLAUDE.md` 핫패스 규칙 / `GEMINI.md` | 훅에서 "Win32 호출 금지" / "no Win32 calls" | 차단 시 `PlaySoundW`/`MessageBeep` 호출(같은 문서가 사운드 캐시 재생을 허용하므로 사실상 예외 조항 누락) |
| `CLAUDE.md` 모듈 지도 | "모든 가시 문자열은 한/영 쌍으로 존재" | 업데이트·헬퍼 오류 메시지는 한국어 전용(ISSUE-006) |
| `README.md` 기능 5 (EN) | "helper safely swaps binaries in `%TEMP%`" | `%TEMP%`는 스테이징 위치이고, 교체는 **설치 폴더**의 exe에서 일어남 |
| `README.md` 기능 3 / `model.rs` 주석 | 키별 통계가 "재시작 후에도 유지" | OS 종료·로그오프 경로에서는 저장되지 않음(ISSUE-004) |
| `docs/UPDATE.md` | 헬퍼를 릴리스 자산으로 배포한다고 기술 | 설치된 헬퍼를 갱신하는 경로는 없음. 후속 업데이트가 버전 불일치로 실패함(ISSUE-001). 사용자 수동 조치도 문서화되어 있지 않음 |

---

## 7. Recommended Fix Plan

### Phase 1 — Immediate
1. **ISSUE-001:** 헬퍼 호환성 판정을 프로토콜 버전 기반으로 바꾸거나, 새 앱이 헬퍼를 자체 갱신하게 한다. 다음 릴리스 **전에** 반드시 처리해야 한다. 이미 배포된 0.2.0 헬퍼가 0.3.0 앱을 거부하지 않으려면, 0.3.0 앱 쪽 검사를 완화하는 방식이어야 한다.
2. **ISSUE-003:** 헬퍼 실행 직후 곧바로 종료하도록 순서를 바꾸고(안내는 Yes/No 확인에 통합), `--timeout-secs`를 명시한다.
3. **ISSUE-002:** 훅을 전용 스레드로 분리한다(구조 변경이지만 핵심 기능의 무결성 문제라 Phase 1에 둠). 응답없음 원인 추적과 독립적으로 효과가 있다.

### Phase 2 — Stability
4. **ISSUE-004:** `WM_ENDSESSION` 처리를 추가하고, 변경이 있을 때만 주기 저장한다.
5. **ISSUE-005 + G3:** `persist_live`를 디스크 재읽기 병합 방식으로 바꾸고, `on_save`에서 `per_key_ms`를 즉시 적용한다.
6. **G1/G2:** 훅 설치 실패와 자동 시작 등록 실패를 사용자에게 표시한다(기존 `save_failed`와 같은 패턴).
7. **G4:** 기동 시 `%TEMP%\kcfix-updater_helper_*.exe` 중 오래된 것을 정리한다.
8. `ShellExecuteW`와 사운드 경로 검증을 UI 스레드 밖으로 옮긴다(G7).

### Phase 3 — Structural
9. **ISSUE-006:** `UpdateError`를 코드 enum으로 바꾸고 i18n 매핑을 적용한다. 헬퍼에 언어 인자를 전달한다.
10. `wndproc` 분기를 "메시지 → 액션" 순수 함수로 추출해 수명주기(종료, 세션 종료, 업데이트 종료) 동작을 HWND 없이 테스트할 수 있게 한다.
11. `persist_live`/`on_save`가 공유하는 "디스크 기준 + 앱 소유 필드 병합" 함수를 `config` 쪽 순수 함수로 만든다.
12. 문서 정정: README 테스트 수, `hook.rs` 주석, CLAUDE/GEMINI 핫패스 규칙의 사운드 예외, UPDATE.md의 헬퍼 갱신 정책.

---

## 8. Test Recommendations

### Unit
- **헬퍼 호환성(ISSUE-001):** `helper_stamp_compatible`을 `(stamp, app_version)` 인자를 받는 순수 함수로 바꾼 뒤 `("0.2.0.0", "0.3.0") → true`(새 정책), `("9.0.0.0", "0.3.0") → false`(미래 헬퍼 거부 여부는 정책에 따라)를 검증한다.
- **설정 병합(ISSUE-005):** 디스크에 `key.41=120`, `iThreshold=70`이 있고 메모리 스냅샷이 `iThreshold=50`·카운터 10일 때 병합 저장 결과가 `key.41=120`을 유지하고 카운터는 10이어야 한다. threshold의 우선순위는 정한 정책(디스크 우선 또는 메모리 우선)대로 검증한다.
- **메시지 → 액션(ISSUE-004):** `WM_ENDSESSION(wParam=1)`은 `Persist`, `WM_ENDSESSION(wParam=0)`은 `None`, `WM_QUERYENDSESSION`은 `TRUE` 반환을 검증한다.
- **update 인자(ISSUE-003):** `launch_helper`의 인자 조립을 분리해 `--timeout-secs <N>`(N ≥ 120)이 포함되는지 스냅샷으로 검증한다.
- **i18n(ISSUE-006):** 모든 `UpdateError` 변형에 대해 `strings(Ko)`/`strings(En)` 매핑 결과가 비어 있지 않고 서로 다른지 검증한다.

### Integration
- **헬퍼 연속 업데이트:** 임시 폴더에 `app.exe`(스탬프 0.3.0 가정)와 옆에 `kcfix-updater.exe`(스탬프 0.2.0)를 두고 `resolve_helper`가 Ok를 반환하는지 확인한다(새 정책 기준).
- **대기 시간 초과 정책:** 헬퍼 `update_executable_with`를 이미 종료된 pid와 `timeout=1s`로 호출하면 즉시 진행되는지 확인한다(기존). 살아 있는 pid로 `timeout=1s`를 준 뒤 그 pid를 종료하면, 새 정책에서 이전 버전 재실행 여부가 기대대로인지 확인한다.
- **저장 실패 표면화:** `APPDATA`를 파일로 막은 상태에서 `on_save` 등가 순수 함수가 `written=false`를 반환하는지 확인한다(기존 `save_reports_failure_when_appdata_is_unusable` 확장).

### End-to-End(수동, `APPDATA` 임시 폴더)
- **연속 업데이트:** 테스트 서명키로 N → N+1 → N+2 매니페스트를 로컬 HTTP나 포크 릴리스에 올리고, 두 번 연속 인앱 업데이트가 성공해야 한다.
- **늦은 확인:** 업데이트 "적용 중" 대화상자를 60초 방치한 뒤 OK를 누르면, 새 정책에서 업데이트가 적용되거나 이전 버전이 실행 중이어야 한다(필터 미실행 상태 금지).
- **종료 경로:** 키 50회 입력 → `shutdown /l` → 재로그인 → 홈 탭 총 입력 수 ≥ 50.
- **파일 편집 보존:** 실행 중 `key.41=120` 추가 → 트레이 종료 → 파일에 `key.41=120`이 남아 있어야 한다.

### Concurrency
- **훅 스레드 분리 후:** UI 스레드에서 2초 sleep을 주입하는 동안 다른 앱에 실제 키 입력을 하면 입력 지연이 없어야 하고, sleep 이후에도 채터링 차단 카운터가 계속 증가해야 한다.
- `SHARED` 락: 훅 스레드와 UI 스레드가 동시에 `decide`/`set_log_limit`를 수행해도 로그 버퍼 용량이 늘지 않는지 확인한다(`log_events.capacity()` 불변, 단위 테스트에서 2스레드 × 10만 회).

### Regression
- 기존 `debounce` 경계 테스트(임계값 정확히 일치 = 통과, 1ms 미만 = 차단, 오토리핏 보존)는 훅 스레드 분리 뒤에도 모두 유지되어야 한다.
- `torn_main_file_falls_back_to_backup`은 병합 저장 도입 후에도 유지되어야 한다.

### Platform-specific
- Windows 10 21H2 / Windows 11 24H2에서 `LowLevelHooksTimeout` 기본값으로 ISSUE-002 재현과 수정을 확인한다.
- 설치 폴더 `C:\Program Files\...`(쓰기 불가)에서 업데이트하면 `BackupFailed` 뒤 이전 버전이 재실행되고 오류 안내가 영어/한국어로 표시되어야 한다.
- 125%/150% DPI, 다크/라이트 전환은 기존 layout 테스트 범위로 충분하다(이번 감사에서 이슈 없음).

---

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | **Acceptable** | 필터 핵심과 설정·서명 검증은 정확하다. 자동 업데이트가 1회용인 결함(ISSUE-001)과 키별 임계값 적용 경로의 공백(G3)이 있다. |
| Runtime Stability | **Needs Work** | 훅이 UI 스레드에 묶여 있어 UI 정지가 곧 시스템 입력 지연과 필터 소실로 이어질 수 있다(ISSUE-002). 응답없음 이력의 원인도 확정되지 않았다. |
| Data Integrity | **Acceptable** | 원자 저장과 `.bak` 폴백으로 파일 손상 경로는 없다. 다만 종료 경로 미저장(ISSUE-004)과 외부 편집 덮어쓰기(ISSUE-005)가 있다. |
| Error Resilience | **Acceptable** | 업데이트는 서명·해시·롤백·이전 버전 재실행까지 견고하다. 대기 시간 초과 경로(ISSUE-003), 훅·자동 시작 실패 미표시(G1/G2)가 남아 있다. |
| Cross-platform Robustness | **Acceptable** | 의도적인 Windows 전용이다. 경로 인용, UTF-16 인코딩, 레거시 경로 폴백이 처리되어 있다. 크로스플랫폼은 목표가 아니므로 감점하지 않았다. |
| Test Confidence | **Acceptable** | 110 + 7개 모두 통과하고, 순수 로직 커버리지가 높다. 수명주기(세션 종료, 업데이트 연속성, 훅 스레드)는 테스트로 보호되지 않는다. 이번 이슈들이 모두 그 공백에 있다. |

### 실제로 먼저 수정할 문제 3개
1. **[ISSUE-001]** 헬퍼 버전 검사 정책 수정 또는 헬퍼 자체 갱신 — 다음 릴리스 전에 처리하지 않으면 그 릴리스 이후 인앱 업데이트가 전부 실패한다.
2. **[ISSUE-002]** 저수준 훅을 전용 스레드로 분리 — 필터가 보이지 않게 꺼지는 경로를 원천 차단하고, 응답없음 현상의 피해 범위를 UI로 한정한다.
3. **[ISSUE-003]** 업데이트 종료 순서 수정(헬퍼 실행 직후 종료, `--timeout-secs` 명시) — 업데이트 중 필터가 실행되지 않는 상태로 남는 경로를 제거한다.

---

## 10. 적용 내역 (2026-10-17, 감사 후속 수정)

| 항목 | 상태 | 변경 |
|---|---|---|
| ISSUE-001 헬퍼 1회용 | **해결** | `src/updater/staging.rs`: 정확한 버전 일치 대신 `ProductName == "Keyboard Chatter Fix Updater"` + 버전 `[HELPER_MIN_VERSION(0.2.0), 앱 VERSION]` 범위로 판정(`helper_compatible`, `file_stamp`). 헬퍼 CLI는 추가만 가능하다는 계약을 `docs/UPDATE.md`에 명문화 |
| ISSUE-002 훅이 UI 스레드에 있음 | **해결** | `src/input/hook.rs`: `spawn_hook_thread` — 메시지 펌프만 도는 전용 스레드(우선순위 HIGHEST)에서 설치·해제. 창 생성 전에 설치하고, 메시지 루프가 끝난 뒤 `HookThread::stop`으로 해제. 독(poison)된 락에서도 패닉 없이 계속 필터링 |
| ISSUE-003 업데이트 대기 시간 초과 | **해결** | `src/ui/update.rs`: "적용 중" 모달 제거(재시작 안내는 Yes/No 확인에 통합). 헬퍼 실행 직후 곧바로 `WM_UPDATE_EXIT`. `helper_args`가 `--timeout-secs 120`을 항상 전달 |
| ISSUE-004 세션 종료 시 미저장 | **해결** | `src/ui/window.rs`: `WM_QUERYENDSESSION` 허용, `WM_ENDSESSION(TRUE)`에서 `persist_live` + 트레이 제거(`session_reply`). `settings_form::autosave_tick`: 5분마다 카운터·보호 상태가 바뀐 경우에만 저장 |
| ISSUE-005 외부 편집 덮어쓰기 | **해결** | `Settings::adopt_file_only_fields` + `config::try_load`: `persist_live`가 저장 직전 디스크의 `key.XX`를 다시 읽어 병합. 파일을 읽지 못하면 메모리 값을 유지(삭제로 오인하지 않음) |
| ISSUE-006 업데이트 오류 한국어 전용 | **해결** | `UpdateError::Failed { kind: UpdateErrorKind, detail }` + 한/영 문구 내장, UI는 `err.message(current_lang)`. 헬퍼는 `KCFIX_LANG` 환경 변수(구 헬퍼는 무시)로 언어를 받아 `HelperError::message` 표시 |
| G1 훅 설치 실패 무시 | **해결** | 실패 시 경고 대화상자(`hook_failed`), 상태 표시는 `HOOK_ACTIVE`가 false면 "꺼짐" |
| G2 자동 시작 실패 무시 | **해결** | 실패 시 토글을 실제 레지스트리 상태로 되돌리고 경고(`autostart_failed`) |
| G3 키별 임계값 Save 미적용 | **해결** | `on_save`가 디스크의 `per_key_ms`를 디바운서에 즉시 적용 |
| G4 `%TEMP%` 스테이징 잔재 | **해결** | 기동 시 백그라운드에서 `cleanup_stale_staging`(이름 패턴 `is_staging_file_name`만, 사용 중 파일은 건너뜀) |
| G7 UI 스레드 차단 I/O | **해결** | 사운드 경로 검증은 워커 스레드(`apply_sound_async`, 세대 번호로 경합 방지), `ShellExecuteW`는 COM을 초기화한 워커 스레드 |
| G5 훅 생존 감시 | 미적용 | 전용 스레드로 타임아웃 원인이 사라져 우선순위를 낮춤. 외부 요인(보안 SW 등)에 의한 제거 감지는 남은 과제 |
| G6 `VK_PACKET`(주입 유니코드) | 미적용 | 기본 설정(`bExcludeInjected=1`)에서는 해당 없음. 추정 사항이라 동작 변경 보류 |
| G8 `--minimized` 미해석 | 미적용 | 넘기는 쪽이 없어 무해 |
| G9 1024자 경로 버퍼 | 미적용 | 영향이 매우 제한적 |
| §6 문서 불일치 | **해결** | README 테스트 수 고정 표기 제거·`%TEMP%` 설명 정정, `hook.rs` 주석, CLAUDE/GEMINI 핫패스 규칙(사운드 예외·전용 스레드), UPDATE.md 헬퍼 호환성 정책 |

### 검증
- `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`: 통과
- `cargo test --workspace`: **123 passed**(110 → 123). 새 테스트:
  `helper_compatible_accepts_older_supported_helpers`, `helper_compatible_rejects_out_of_range_or_foreign`,
  `file_stamp_reads_version_and_product_name`, `helper_args_pass_exit_timeout_and_hash`,
  `staging_leftovers_are_recognized_by_name_only`, `hook_runs_on_a_dedicated_thread_and_stops`,
  `session_end_persists_only_when_the_session_really_ends`, `autosave_waits_for_the_interval`,
  `autosave_skips_when_nothing_changed`, `external_per_key_edit_survives_persisting_the_startup_snapshot`,
  `removed_per_key_edit_is_not_resurrected`, `try_load_distinguishes_missing_from_empty_settings`,
  `every_kind_has_distinct_korean_and_english_text`, `message_appends_detail_in_both_languages`,
  `classify_renders_failure_in_ui_language`, `helper_lang_defaults_to_korean_and_localizes_english`
- 디버그 바이너리를 임시 `APPDATA`로 4초간 기동해 창 생성, 응답(Responding=True), 정상 종료를 확인

### 남은 수동 검증(실행하지 않음)
- 실제 릴리스 N → N+1 → N+2 연속 인앱 업데이트 E2E
- 로그오프·재로그인 후 통계 보존(`WM_ENDSESSION` 경로)
- 실제 키보드로 채터링 차단 동작 확인(훅 스레드 분리 후)

> 후속(2026-10-17): 위 수동 검증 중 연속 업데이트(게시된 v0.2.0 헬퍼 포함)·세션 종료 저장·UI 정지 중 훅 동작은
> `scripts/e2e`로 실측해 통과했다. 실제 물리 키보드·실제 로그오프·실제 GitHub 게시 경로는 여전히 수동 항목이다.
> 세부와 추가로 발견·수정한 회귀(UI-001)는 `PROJECT_AUDIT_UI_TESTS.md` 참조.

> v0.2.1에서 §10의 "미적용" 중 G5(훅 생존 감시)를 제외한 후속 항목과 `PROJECT_AUDIT_UI_TESTS.md`의
> 모든 수정 대상이 반영되었다. 릴리스 절차는 `docs/UPDATE.md` 참조.

---

## 11. v0.2.2 — 언어 자동 인식과 UI 다듬기

**언어(i18n)**
- 설정 `sLanguage`: `auto`(기본) / 언어 코드. 자동은 Windows 표시 언어의 첫 항목(`GetUserPreferredUILanguages`, 실패 시 `GetUserDefaultLocaleName`)이 한국어면 한국어, 그 외는 영어. v0.2.1 이하가 저장한 `ko`/`en`은 고정으로 유지된다(파일만으로는 사용자가 고른 값인지 기본값인지 구분할 수 없음 — 설정 > 언어에서 "자동"을 고르면 된다).
- 확장 구조: `Language::ALL` 레지스트리(`code`/`native_name`/`from_code`), `LanguagePref { Auto, Fixed }`. 모든 `match`가 exhaustive여서 새 언어 추가 시 컴파일러가 누락 지점을 알려준다. 선택기는 "자동 + 등록된 모든 언어"를 자동 생성. 헬퍼는 모르는 언어 코드에 영어를 사용. 절차는 `src/i18n/language.rs` 상단 주석.
- 검증: `strings.rs`가 모든 언어를 영어 기준으로 검사(빈 문구, 플레이스홀더 수, 목록 길이, 긴 문구의 번역 누락; `define_strings!` 매크로가 필드 목록을 자동 생성), 레지스트리·로케일 매핑·설정 파싱 단위 테스트, `--check-language` 스모크, `scripts/e2e/language_e2e.ps1`(실제 창에서 ko/en/auto/레거시/알 수 없는 값, 비한국어 시스템(`KCFIX_LOCALE`), 선택기 변경의 즉시 번역·저장)
- 진단: `--check-language`, 환경 변수 `KCFIX_LOCALE`(예: `de-DE`)

**UI (실제 창을 캡처해 점검)**
- 기록 탭 목록 헤더 글자가 다크 테마에서 검정(거의 안 보임) → 헤더를 서브클래싱해 직접 그림(리스트뷰가 헤더의 커스텀 드로우 결과를 무시하는 것을 실측으로 확인).
- 다크 테마에서 메뉴 바만 흰색 → 메뉴 바 전용 그리기 메시지(`WM_UAHDRAWMENU`/`ITEM`)와 하단 1px 선을 다크로 처리(라이트 테마는 시스템 기본).
- 시작 직후 탭 버튼의 점선 포커스 박스 → 키보드를 쓰기 전까지 숨김(`WM_CHANGEUISTATE`).
- 언어 선택기의 "자동" 문구가 잘리던 문제 → 문구 단축.
- 한계: 메뉴 드롭다운·트레이 메뉴는 시스템 기본(밝은) 스타일 유지(문서화되지 않은 uxtheme API 없이는 어두운 팝업 메뉴를 만들 수 없어 보류). 라이트 테마는 캡처로 확인하지 못함(시스템 설정을 바꾸지 않기 위해).

---

## 12. v0.3.0 — 필터 정확도, 키·마우스 탭, 자동화·안정성 기능

세부 목록은 `CHANGELOG.md`, 제안과 항목별 결과는 `docs/IMPROVEMENTS.md`.

**검증**
- 단위 테스트: 필터 코어(누르는 중 바운스, IME 코드, 확장 키 분리, 떼기 확인의 보류·취소·늦은 방출, 무작위 스트림 불변식 2종), 권장값 계산, 단축키 파싱, 앱 목록·전체 화면 판정, 워치독 판단, CSV, 작업 XML, 자동 실행 항목 갱신 조건, 레이아웃(5탭, 카드·겹침).
- 실제 바이너리 E2E: 기존 `hook_session`·`language`·`update_swap` 통과, 신규 `features_e2e`(떼기 확인·마우스 X2·단축키·자동 일시 중지·카운터 저장) 통과.
- 창 캡처(한/영, 150% 배율)로 새 탭·카드 배치 확인, 잘린 문구 2건 수정.

**개발 중 발견해 고친 문제**
- 키보드 raw input 등록(워치독 초안)이 앱 창이 전경일 때 LL 훅 입력을 끊음 → raw input 폐기, 프로브 방식으로 교체. E2E가 이 회귀를 잡도록 창을 띄운 상태로 실행.
- 자동 실행 항목 갱신 초안이 테스트 빌드 실행만으로 실제 Run 항목을 테스트 exe로 바꿈 → "같은 실행 파일을 가리키는 옛 항목"만 갱신하도록 제한.

**남은 수동 항목**: UAC 동의가 필요한 관리자 권한 재실행·작업 등록, 실제 훅 제거 상황의 워치독 복구, winget 제출.
