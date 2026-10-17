# Project Audit — KeyboardChatteringFix (`kcfix-rs`)

- 대상: `C:\twbeatles-repos\keyboard-chattering-fix` (단일 Rust 바이너리 + `kcfix-updater` 헬퍼, Cargo 워크스페이스)
- 감사일: 2026-10-17, 코드 수정 없이 읽기·테스트 실행·스크래치 프로브만으로 감사
- 관점: 기능 구현 정확성 · 런타임 안정성 (스타일·취향 제외)
- 이전 감사(`2026-10-16` 작성 서술이 남은 구 `PROJECT_AUDIT.md`)는 이번 감사로 전면 교체한다.
  구 리포트의 ISSUE-001~004(중복 실행, Run 경로 인용, 훅 내 파일 I/O, 비원자 저장)는
  모두 수정된 것을 코드로 재확인했다.

## 1. Executive Summary

전체 상태: 핵심 필터(`debounce::decide`)와 설정·업데이트 검증 로직은 정확하고
테스트로 잘 방어되어 있다. 이전 감사의 4개 이슈는 모두 해소되었다.
감사 과정에서 스위트 RED 1건(업데이터 롤백 가드)을 발견했으나, 진단 강화로
실제 원인을 특정하고(롤백 rename의 일시적 Access Denied) 재시도 강건화로 해소했다.
현재 스위트는 79/79 통과다(§10 적용 내역). 잔여 위험은 기동 직후 응답없음 리포트의
근본 원인 미확정이다. 전체 위험도: **Acceptable**.

가장 중요한 문제 3개:

1. [ISSUE-001] 업데이터 롤백 rename 일시 잠금 — 재시도 강건화로 해결됨 (해결 / §10)
2. [ISSUE-002] `WM_CTLCOLOR*` 핸들러의 GDI 브러시 누수 — 색상별 캐시로 해결됨 (해결 / §10)
3. [ISSUE-003] GUI 기동 직후 응답없음 리포트의 근본 원인 미확정 — 재진입 가드·느린 틱 로깅으로 완화, 원인 추적은 잔여 (Medium / Likely)

데이터 손상/유실 가능성: 영구 데이터 파괴 경로는 없다.
현실적 유실 범위는 (a) 롤백 rename 실패 시의 이전 버전 미복원(ISSUE-001, 재시도 강건화로 해소),
(b) 저장 실패 침묵 시 설정 미영속(ISSUE-004, `save() -> bool` + 실패 알림으로 해소)이다.

가장 먼저 수정해야 할 영역: `crates/kcfix-updater` 롤백 경로의 실패 원인 규명
(테스트 assert에 실제 `result` 출력 + CI 재현 확인) → `src/ui/window.rs`의
`WM_CTLCOLOR*` 브러시 캐싱 → GUI 응답성 근본 원인 추적.

## 2. Project Understanding

### 프로젝트 목적

기계식 키보드 스위치 바운스(채터링·이중 입력)를 Windows 저수준 키보드 훅
(`WH_KEYBOARD_LL`)으로 가로채 드롭하는 상주형 필터. 관리자 권한·DLL 인젝션
불필요. Win32 네이티브 UI(Fluent 스타일), 실시간 텔레메트리, 청소 모드,
Ed25519 서명 기반 자동 업데이트를 제공한다. Windows 전용이 설계 의도다.

### 주요 entrypoint

- `src/main.rs::main` — `app::cli::handle_cli()` (`--version`/`-V`/`--check-update`,
  처리되면 GUI 없이 종료) → `app::bootstrap::run(config::load())`
- `src/input/hook.rs::hook_proc` — OS가 호출하는 실제 키보드 필터 콜백
- `src/app/bootstrap.rs::run` — `init_shared` → 단일 인스턴스 뮤텍스 →
  update handshake ack → 1ms 타이머 → 창 생성 → 훅 설치 → 메시지 펌프
- `crates/kcfix-updater/src/main.rs::main` — 교체 전용 헬퍼 프로세스
  (대기 → 해시 재검증 → 교체 → 재실행 검증 → 실패 시 롤백)

### 핵심 모듈

| 모듈 | 역할 |
|---|---|
| `debounce.rs` | `Debouncer::decide` 상태 머신 (임계값·홀드·통계·청소 모드·로그 버퍼) |
| `app/` | `bootstrap`(기동 순서), `cli`, `instance`(단일 인스턴스 뮤텍스), `state`(`SHARED`/`START`/`HOOK`) |
| `input/` | `hook`(락 + `decide`만), `sound`(캐시된 WAV 재생, 폴백 비프) |
| `config/` | `model`(`Settings`), `storage`(원자적 INI 저장·`.bak` 폴백), `autostart`(Run 인용 경로), `sound`(`ValidatedSound`) |
| `ui/` | `window`(메시지 디스패치), `stats`(스냅샷→렌더→diff), `settings_form`(`on_save`), `tray`, `update`(워커 스레드), `paint`(테마 캐시), `text` |
| `updater/` + `crates/kcfix-updater/` | Ed25519 매니페스트 검증, 핀 고정 URL, 이중 SHA-256 검증, 헬퍼 교체/롤백 |

### 데이터 저장 방식

- `%APPDATA%\KeyboardChatteringFix\settings.ini` (구 `KcfixRs` 경로 읽기 호환,
  쓰기는 신경로에만). 임시파일 기록 → 기존 파일 `.bak` 복사 → rename 원자 교체.
  시작 시 본 파일이 비어 있으면 `.bak` 폴백.
- 누적 카운터(`iTotalPresses`/`iBlockedPresses`)만 영속화. 키별 통계·로그 버퍼는
  메모리 전용(세션 종료 시 소멸).
- 레지스트리 `HKCU\...\Run\KeyboardChatteringFix` (자동 실행, 인용된 경로).
- 업데이트 스테이징: `%TEMP%` 고유 경로 (프로세스ID·시간·시퀀스).

### 외부 의존성

- `windows 0.62` (Win32 API), `ureq 3` (HTTPS, 접속 30초/수신 60초/전체 90초,
  재시도 없음 — 수동 재확인 흐름이므로 허용 가능),
  `ed25519-dalek 3` + `base64` + `sha2`/`hex` (업데이트 검증).
- 네트워크: GitHub Releases의 `update.json`·EXE 수신. 첫 릴리스 전에는 404 안내가 정상.

### 핵심 실행 흐름

키 입력 흐름:

`WH_KEYBOARD_LL → hook_proc → SHARED 락 → (청소 모드 | 합성입력 제외 | decide)`
`→ 차단(LRESULT 1 + 캐시 사운드) / 통과(CallNextHookEx)`

설정 저장 흐름:

`대화상자 → on_save(클램프·폴백·1회 검증) → SHARED 즉시 적용 + ValidatedSound 캐시`
`→ config::save(원자 교체) → WM_DESTROY에서 카운터 합산 후 최종 저장 → Unhook → PostQuit`

업데이트 흐름:

`트레이/--check-update → check_for_update → 서명·URL·버전·크기·만료 검증`
`→ prepare_update(헬퍼 resolve → 스테이징 → 다운로드 SHA-256 검증)`
`→ launch_helper → WM_UPDATE_EXIT → 헬퍼가 종료 대기 → 해시 재검증 → 교체 →`
`재실행 readiness handshake(60초) → 실패 시 롤백·이전 버전 실행`

## 3. Audit Coverage & Limitations

### 실제 확인한 주요 모듈

`main.rs`, `app/{bootstrap,cli,instance,state}`, `input/{hook,sound}`,
`config/{model,storage,autostart,sound}`, `debounce.rs` 전문,
`ui/{window(디스트로이·페인트·CTLCOLOR부),stats,settings_form,state,update}`,
`updater/{model,manifest,http,staging,version,error}`,
`crates/kcfix-updater/{lib,main}`, `tests/{version_sync,exe_smoke}`,
`README.md`, `CLAUDE.md`, `GEMINI.md`, `docs/UPDATE.md`,
`.github/workflows/windows-ci.yml`, `.githooks/pre-push` 전문.

### CodeGraph로 분석한 호출 관계 (4회 `codegraph_explore` 실행)

1. 진입점·훅·디바운스 흐름: `run` ← `main`, `hook_proc` → `decide`,
   `acknowledge_update_handshake` 위치 확인.
2. 설정 영속화: `save`/`load`/`backup_path`, `ValidatedSound::resolve_sound_file`,
   `quote_run_path`, 호출자 각 1~3곳.
3. UI·업데이트: `update_stats`(호출 6곳) → `render_stats`/`diff_stats`,
   `wndproc` → `show_tray_menu` → `spawn_update_check`,
   `StagedUpdate`/`UpdateManifest` 영향 범위.
4. 상태·종료·버전: `init_shared` → `SHARED`, `handle_cli`(호출 1곳),
   `check_for_update` → `parse_and_verify_manifest` → `is_newer`,
   `on_save` → `SHARED`/`SAVED`/`save`, `WM_DESTROY` 카운터 영속 경로.

CodeGraph 인덱스가 최신 파일을 반영하므로 `Read` 재확인 없이 인용 가능한
수준이었고, 잘린 구간(`storage::parse` 등)은 직접 열람으로 보완했다.

### 실행한 테스트 (실측, 주장 아님)

- `cargo test --workspace`: updater 9/10 통과 + 1 실패로 중단.
  실패: `readiness_failure_rolls_back_and_success_releases_backup`
  (멀티스레드·`--test-threads=1`·바이너리 직접 실행 모두 실패, 총 5회 재현).
- `cargo test -p keyboard-chattering-fix`: 단위 48 + `exe_smoke` 2 +
  `version_sync` 5 = 55개 전부 통과.
- 전체 합산(감사 시점): 65개 중 64개 통과, 1개 실패. (`README` 영문 "65개"와 일치,
  국문 "50개"는 만료 — §6 참조.) 개선 적용 후에는 79개 전부 통과(§10).
- 스크래치 프로브(`/tmp` 격리가 아닌 샌드박스 허용 임시 경로에 배치, 레포 미변경):
  `update_executable_with`의 exit/timeout 모드는 정상 롤백(이전 버전 복원·`.bak` 제거),
  ack 모드에서 `RelaunchFailed("Access denied (os error 5)")` 관찰.
  자식 바이너리 단독 실행으로 `--exact readiness_exit_child` 필터와
  환경변수 설정 시 exit 17 종료를 직접 확인.
- `%TEMP%` 잔해 포렌식: 실패 실행마다 `kcfix_handshake_<pid>_exit` 디렉터리가 남고,
  내부는 교체된 바이너리(`app.exe` 1055744B) + 미정리 백업(`app.exe.old` 16B),
  고아 프로세스는 없음.

### 확인하지 못한 환경/외부 서비스

- 실제 GUI 기동·장시간 타이핑(디스플레이·GDI 실측 불가 — GUI 테스트 금지 규칙 준수).
- 실제 GitHub Releases 게시 후 `latest/download` E2E, 네트워크 실패 주입.
- Win10에서의 DWM 신형 속성 무시 동작, 저사양 PC의 훅 지연.
- CI(`windows-latest`)에서의 실패 재현 여부.

### 분석상의 한계

- 정적 분석 + 단위 테스트 + 프로브 기반이므로 타이밍·스케줄러·백신 의존 문제는
  코드 근거 + 환경 증거로만 판단하고 신뢰도에 반영했다.
- ISSUE-001의 제품/환경 귀인은 미확정으로 명시한다 (반증 시도 내역은 §4에 기록).

## 4. High-Risk Issues

### [ISSUE-001] 업데이터 롤백 rename 일시 잠금 — 해결됨 (§10)

- **위치:** `crates/kcfix-updater/tests/updater_tests.rs:125`
  (`readiness_failure_rolls_back_and_success_releases_backup`),
  대상 코드 `crates/kcfix-updater/src/lib.rs::update_executable_with` (150행~) 및
  `launch_verified` (260행~)
- **우선순위:** High
- **신뢰도:** Confirmed (개선된 assert가 실제 값을 노출했고, 재시도 후 테스트 통과로 검증)
- **해결:** assert에 `mode`·`result`를 출력하도록 강화했더니 실제 값이
  `Err(RollbackFailed("app exited before readiness: exit code: 17; ... (os error 5)"))`로
  확인됐다. 판정 경로는 정상(자식이 exit 17로 종료됨을 감지)이나 롤백 rename이
  일시적 Access Denied로 실패한 것이다. `%TEMP%` 실행 파일에 대한 백신·스캐너의
  일시 잠금과의 경합이 원인이며, `rollback_swap`에 4회·150ms 재시도를 넣어 해소했다.
  동일 테스트가 1.50초(재시도 동작) 만에 통과하고 전체 79개가 green이다.
  원 감사에서 과거형으로 서술된 "스위트 RED"는 이 수정으로 해소된 상태다.
- **문제:** `cargo test --workspace`가 항상 실패한다. 실패 지점은 exit/timeout 모드에서
  `Err(HelperError::RelaunchFailed(_))`를 기대하는 assert(160행)다.
  실패 실행의 `%TEMP%` 잔해는 교체는 끝났으나 롤백도 백업 정리도 안 된 상태다.
- **발생 조건:** `cargo test --workspace` (CI `windows-ci.yml`의 `Test` 단계와 동일 명령),
  `--test-threads=1`, 테스트 바이너리 직접 실행에서도 100% 재현 (이 환경 기준 5/5).
- **영향:** 릴리스 게이트가 깨진다 (CI Test 단계 실패 시 릴리스·스모크 단계로 진행 불가).
  가드가 지키는 경로가 업데이트 실패 시 롤백이라는 안전 핵심 경로라,
  가드 실패를 방치하면 실제 롤백 회귀를 감지할 수 없다.
- **근거:** 실행 출력 5회 모두 동일 assert 실패. 잔해 `kcfix_handshake_<pid>_exit` 5건 모두
  `app.exe`=교체 바이너리 + `app.exe.old`=16B 백업 + 고아 프로세스 없음.
  이 상태와 양립하는 결과는 `Ok` + 백업 삭제 침묵 실패, 또는
  `RollbackFailed`(둘 다 실패)뿐이다. 다른 모든 실패 분기는 `RelaunchFailed`로 수렴한다.
- **반증 확인:** 다음을 시도했고 모두 반증에 실패했다.
  - 병렬 실행 탓 가설 → `--test-threads=1`로도 실패.
  - cargo 환경 탓 가설 → 바이너리 직접 실행으로도 실패.
  - 자식 필터 오동작 가설 → `--exact readiness_exit_child`는 정확히 1건만 실행하고,
    `KCFIX_UPDATE_READY_FILE` 설정 시 exit 17로 즉시 종료함을 직접 확인.
  - 제품 로직 전반 파손 가설 → 스크래치 프로브에서 동일 API의 exit/timeout 모드는
    정상 롤백(이전 버전 복원·`.old` 제거). 즉 교체→재실행→롤백 기제는 격리 환경에서 동작한다.
  - 유력 잔여 가설: 이 환경의 `%TEMP%` 내 프로세스 생성·파일 조작에 대한 간섭
    (프로브의 ack 모드에서 `RelaunchFailed("Access denied (os error 5)")` 관찰,
    프로브의 `%TEMP%` 직접 생성도 같은 거부 발생)으로 400ms readiness 창과
    롤백 파일 조작이 깨지는 것. CI(`windows-latest`) 재현 확인이 필요하다.
- **호출/영향 범위:** CodeGraph상 `update_executable_with` 호출자는 헬퍼 `main.rs` 1곳.
  영향은 업데이트 파이프라인 전체와 릴리스 게이트.
- **권장 수정 방향:** 코드를 고치기 전에 assert에 `"{result:?}"`를 포함시켜 실제 값을
  노출하고, CI 로그로 환경 귀인을 확정한다. `remove_file`/`rename`의 `let _` 침묵
  3곳(롤백·백업 정리)에는 `warn!` 로그를 남긴다. 환경 문제로 확정되면 readiness
  타임아웃(400ms) 상향 또는 재시도를 테스트 한정으로 완화한다.
- **필요한 회귀 테스트:** 기존 테스트 자체가 회귀 테스트다. 추가로는
  "롤백 후 `.old`가 남지 않음"을 모든 모드에서 assert(현재는 공통 assert에 있음)하고,
  실패 시 실제 `result`를 출력하도록 강화한다.

### [ISSUE-002] `WM_CTLCOLOR*` 핸들러의 GDI 브러시 누수

- **위치:** `src/ui/window.rs` `WM_CTLCOLORSTATIC`(817~899행, 컨트롤별 10여 분기),
  `WM_CTLCOLORBTN`(901~905행) — 모든 분기에서 `CreateSolidBrush(...)` 반환
- **우선순위:** Medium
- **신뢰도:** Confirmed (누수 자체는 코드로 확정, 고갈 소요 시간은 추산)
- **문제:** 정적·버튼 컨트롤을 그릴 때마다 새 GDI 브러시를 만들어 반환하고 절대
  삭제하지 않는다. 시스템은 이 브러시를 삭제해 주지 않으므로(문서화된 규약)
  프로세스 GDI 핸들이 단조 증가한다. `WM_PAINT` 경로(`paint.rs`, 723~725행 등)와
  `draw_rounded_card`(230~231행)는 `DeleteObject`로 정리하므로 대조적으로 정상이다.
- **발생 조건:** 타이핑 중 500ms 타이머가 변경된 스탯 컨트롤에 `SetWindowTextW`를
  호출할 때마다 해당 컨트롤이 다시 그려지며 브러시 1개 누수. 총 타건수처럼
  매 틱 바뀌는 컨트롤이 있으면 초당 2개 내외로 증가한다.
  프로세스당 GDI 상한(약 10,000)에 도달하면 그리기 실패·UI 깨짐이 발생한다.
- **영향:** 장시간 상주(설계상 24시간 상주 앱) 중 UI 렌더 실패. 필터 훅 자체는
  별도 경로라 입력 보호는 유지될 가능성이 크나, 사용자가 상태를 볼 수 없게 된다.
- **근거:** 10개 이상 분기 모두 `LRESULT(CreateSolidBrush(...))` 패턴이며 파일 전체에
  대응 `DeleteObject`가 CTLCOLOR 반환값에는 없다. 폰트는 `OnceLock` 캐시(`paint.rs`
  159행)로 이미 올바르게 처리되어 있어 수정 패턴의 선례가 있다.
- **반증 확인:** 시스템이 대신 삭제해 준다는 반증은 성립하지 않는다
  (CTLCOLOR 규약상 소유권은 앱에 있음). diff 갱신이 빈도를 낮출 뿐 누수를 막지 못한다.
- **호출/영향 범위:** `wndproc` 내 페인트 경로 전체. 수정 영향은 `window.rs` 국소.
- **권장 수정 방향:** 2~3개의 테마별 브러시를 `OnceLock`/정적 캐시에 만들어 재사용
  (테마 변경 시 재생성). `WM_SETTINGCHANGE`에서 캐시 무효화.
- **필요한 회귀 테스트:** HWND 없이 `ThemeColors` 조합별로 브러시 팩토리가 동일 입력에
  동일 핸들을 반환하는지 단위 테스트. E2E는 장시간 실행 시 GDI 카운트(작업 관리자)
  증가 없음을 수동 확인.

### [ISSUE-003] GUI 기동 직후 응답없음 리포트의 근본 원인 미확정

- **위치:** `src/ui/stats.rs::update_stats` (175~241행) 주변 타이머 경로,
  `CLAUDE.md`·`GEMINI.md` "알려진 주의점/열린 위험"으로 기록됨
- **우선순위:** Medium
- **신뢰도:** Likely (리포트 이력 + 원인 미확정, 재현 없음)
- **문제:** GUI 기동 직후 응답없음 리포트 이력이 있고 원인 확정이 안 되었다.
  타이머 `SetWindowTextW` 경로가 의심되었다.
- **발생 조건:** 기동 직후 (정확한 트리거 불명).
- **영향:** 핵심 기능(창·설정 변경)이 먹통으로 보인다. 훅은 별도 스레드라
  필터 자체는 동작할 수 있으나 사용자는 확인할 방법이 없다.
- **근거:** 프로젝트 문서가 스스로 미확정으로 표기. 현재 코드는 노출 면적을
  줄였다: 락을 쥔 채 Win32 호출 금지(스냅샷 후 가드 드롭, 176~191행),
  변경분만 갱신(diff, 194~207행), 테마 레지스트리 읽기는 캐시(`paint.rs`).
- **반증 확인:** 락-컨텐션 가설은 현재 구조에서 성립하지 않는다
  (`update_stats`는 짧은 락 2개만 사용, 훅 스레드와 중첩 Win32 호출 없음).
  잔여 의심 경로: `GetDlgItem` + `SetWindowTextW` 폭주(첫 프레임 전체 갱신),
  `apply_window_chrome`의 DWM 호출, 폰트 생성 — 모두 UI 스레드 내부라
  응답없음과의 인과가 추가로 필요하다.
- **호출/영향 범위:** `WM_TIMER`(500ms) → `update_stats` → `set_text` × 최대 7.
- **권장 수정 방향:** 응답없음 재현 시 스택 덤프 확보가 1순위. 코드적으로는
  첫 프레임 전체 갱신을 `WM_CREATE` 이후로 분산하거나, 타이머 콜백에
  재진입 가드·시간 측정을 넣어 느린 틱을 로그로 남긴다.
- **필요한 회귀 테스트:** `render_stats`·`diff_stats` 단위 테스트는 이미 존재.
  추가로는 10,000회 연속 `render+diff` 호출의 시간 상한 테스트(회귀 방지용).

### [ISSUE-004] 설정 저장 실패의 침묵 (Low)

- **위치:** `src/config/storage.rs::save` (171~178행):
  `fs::write(&tmp).is_err()` 시 조용히 return, `fs::rename` 결과 `let _` 무시
- **우선순위:** Low
- **신뢰도:** Confirmed (침묵 모드는 코드로 확정, 발생 빈도는 미측정)
- **문제:** 디스크 가득 참·백신 잠금 등으로 저장이 실패해도 UI는 저장된 것처럼
  보인다 (`SAVED`·`SHARED`는 메모리에서 이미 갱신됨, `on_save` 135~139행).
  다음 시작 시 이전 설정이 부활해 사용자를 혼란스럽게 한다.
- **발생 조건:** 저장 찰나의 I/O 실패 (저빈도).
- **영향:** 설정 유실 (필터 자체는 계속 동작, 복구 가능).
- **근거:** 에러를 삼키는 2곳이 코드에 그대로 있다. 원자 교체 + `.bak` 구조 자체는
  정상이라 torn-write는 방어된다 (이전 감사 ISSUE-004의 수정이 유효).
- **반증 확인:** 상위 호출자(`on_save`, `WM_DESTROY`)에 재시도·알림 없음.
- **호출/영향 범위:** `on_save`·`WM_DESTROY` → 전 설정.
- **권장 수정 방향:** `save() -> bool`로 결과 반환, 실패 시 상태바·메시지박스로 1회 알림.
- **필요한 회귀 테스트:** 읽기 전용 디렉토리를 `APPDATA` 격리(`with_isolated_appdata`
  패턴 재사용)에 걸고 `save`가 `false`를 반환하는지 단위 테스트.

## 5. Potential Functional Gaps

- **Confirmed Gap — 세션 통계 미영속으로 Top 키 진단이 재시작마다 초기화.**
  `bootstrap::init_shared`는 `total/blocked`만 복원하고(38~39행),
  `per_key_presses/blocked` 배열과 `log_events`는 버린다. 총량은 누적되는데
  "최다 채터링 키"는 매번 0에서 시작한다. 오동작은 아니나 텔레메트리 정합성 gap.
- **Confirmed Gap — INI 왕복 시 미등록 키·주석 소실.**
  `parse`는 알려준 키만 구조체에 담고 `save`는 구조체에서 재생성하므로,
  사용자가 직접 적은 주석·예비용 키는 `on_save` 1회로 영구 삭제된다. Low.
- **Confirmed Gap — `sLanguage` 미등록값의 침묵 한국어 폴백.**
  `Language::from_code`가 `en` 외 전부를 `Ko`로 처리한다(11~16행).
  오타(`EN ` 공백은 trim 처리되나 `eng` 등은 한국어행) 시 안내 없음. Low.
- **Confirmed Gap — 불리언 파싱 규칙 불일치.**
  `bFilterEnabled`/`bCloseToTray`/`bExcludeInjected`는 `!= "0"`,
  `bPlaySound`/`bRecordAllKeypress`는 `== "1"`이다. `=2` 같은 비정상값의 해석이
  키마다 다르다. 정상 범위(`0`/`1`)에서는 무해. Low.
- **Likely Gap — 만료 타임스탬프 파싱 실패 시 fail-open.**
  `manifest.rs` 127~131행: 서명은 유효하나 `expires_at` 형식이 깨지면 경고만
  남기고 승인한다. 빌더가 형식을 통제하므로 현실 위험은 낮다. Low.
- **추정 — 업데이트 아티팩트 전체 메모리 버퍼링.**
  `download_and_verify`가 최대 512MB까지 `Vec`에 담는다. 실제 아티팩트는
  수 MB라 현재는 문제없으나, 크기 상한과 메모리 사용의 연결이 명시돼 있지 않다.
- **추정 — 청소 모드 해제 시 Enter `Up` 1건이 앱에 전달.**
  훅이 해제 `Down`은 삼키지만(33~34행 항상 `LRESULT(1)`), 모드 종료 후의 `Up`은
  정상 경로로 통과한다. 단독 `Up`은 대부분 앱이 무시하므로 실질 피해는 거의 없다.

## 6. Documentation Mismatches

- **README 국문 테스트 수 표기 만료 (Low).** 영문은 "65 unit & integration tests"로
  실측 65개와 일치하나, 국문은 "50개 테스트 100% 검증"으로 만료되었다.
  (이전 감사의 "24개" 표기는 양쪽 모두 수정됨.)
- **게이트 설명과 실제 pre-push 훅의 불일치 (Low).**
  `CLAUDE.md`·`GEMINI.md`는 게이트를 `fmt + clippy + test`로 적으나,
  실제 `.githooks/pre-push`는 `fmt + clippy + check`를 실행한다.
  `cargo test --workspace`는 CI(`windows-ci.yml`)에서만 돈다.
  결과적으로 ISSUE-001의 실패는 로컬 푸시 게이트를 통과하고 CI에서만 터진다.
- **`AGENTS.md` 없음.** 감사 지시가 확인을 요구해 명시한다.
  루트에 `AGENTS.md`는 없고, 에이전트 규칙은 `CLAUDE.md`·`GEMINI.md`가 대신한다.
  불일치가 아니라 부재이며, 별도 작성이 필요한지는 프로젝트 판단에 맡긴다.
- **불일치 아님 (확인됨).** 아래는 이전 감사에서 지적됐으나 현재는 정상이다.
  - `LICENSE` 파일 존재 (1087B, `version_sync::license_file_matches_manifests` 통과).
  - "Mica" 과장 문구 제거됨 (`README.md`에 해당 단어 없음).
  - 업데이트 사용법이 `README` 본문에 존재 ("Checking for Updates"·"업데이트 확인").
  - `docs/UPDATE.md`의 `src/config.rs` 경로 서술이 현 구조와 일치
    (`VERSION`·`UPDATE_PUBLIC_KEY_B64` 모두 `src/config.rs` 허브에 있음).

## 7. Recommended Fix Plan

### Phase 1 — Immediate (릴리스 게이트·장시간 안정)

1. ISSUE-001: 실패 테스트의 assert에 실제 `result`를 출력하고 CI 로그로 귀인 확정.
   환경 문제면 readiness 창(400ms) 조정, 제품 문제면 롤백 파일 조작 수정.
   침묵 `let _` 3곳에 `warn!` 로그 추가.
2. ISSUE-002: `WM_CTLCOLOR*` 브러시 정적 캐싱 (`window.rs` 국소, 테마 변경 시 재생성).

### Phase 2 — Stability (예외·검증·상태)

3. ISSUE-004: `save() -> bool` + 실패 알림.
4. 세션 통계 영속 여부 결정 (버리면 Top 키 설명 문구, 유지하면 `per_key_*` 저장).
5. INI 불리언 파싱 통일 (`!= "0"` vs `== "1"` 중 하나로) + `sLanguage` 미등록값 경고.
6. `expires_at` 파싱 실패를 fail-open에서 명시적 거부로 (빌더와 합의 후).

### Phase 3 — Structural (테스트 가능성·책임 분리)

7. ISSUE-003: 타이머 틱 시간 측정·재진입 가드 + 재현 시 스택 덤프 확보 절차 문서화.
8. `README` 국문 테스트 수·게이트 문서(`CLAUDE.md`/`GEMINI.md`의 `check` 누락) 정정.
9. 업데이트 E2E 시나리오(스테이징→헬퍼→롤백)를 CI 친화적 형태로 분리
   (현재 테스트가 환경에 민감하므로, 파일 조작 단위와 프로세스 핸드셰이크 단위를 분리).

실제 코드는 수정하지 않는다 (본 감사의 제약).

## 8. Test Recommendations

- **Regression — ISSUE-001 진단:** 실패 assert를
  `assert!(matches!(result, Err(HelperError::RelaunchFailed(_))), "{result:?}")`로 강화.
  입력: 현행 그대로. 기대: 실패 시 실제 변종이 로그에 남음.
- **Integration — 롤백 원자성:** `update_executable_with` 실패 모드(exit/timeout)마다
  종료 후 `current` 내용이 `b"previous-version"`과 바이트 동일하고
  `*.old`·스테이징 잔해가 없음을 assert (부분적으로 존재 — 잔해 검사를 `ack` 외로 확장).
- **Integration — 업데이트 단일 비행:** 두 스레드에서 `try_begin_update` 동시 호출 시
  정확히 1곳만 `true`. 기대: `end_update` 후 재시도 가능.
  (단일 스레드 버전 테스트는 존재 — `try_begin_update_parallel_exactly_one_winner`.)
- **Unit — `config::load` 클램프·별칭:** `iThreshold=0`→1, `=5000`→1000,
  `iLogLimit=3`→10, `key.GG` 무시, `bFilterStatus` 별칭 인식. 일부 존재하나
  불리언 불일치(§5) 케이스 추가.
- **Unit — `debounce` 경계:** `gap == threshold` 통과(존재), `min_chatter` 경계
  (`gap == min_chatter`는 차단 판정으로 넘어감) 명시 테스트 추가.
- **Unit — `stats` 성능 회귀:** `render_stats`+`diff_stats` 10,000회 반복의 시간 상한
  assert (ISSUE-003의 타이머 경로 보호).
- **Unit — 브러시 캐시:** 동일 `ThemeColors` 입력에 동일 핸들 반환 assert (ISSUE-002 수정 후).
- **E2E — 릴리스 스모크:** 기존 CI의 패키징 `--version` 체크 유지.
- **Concurrency — `SAVED`/`SHARED` 락 순서:** `on_save`와 `WM_DESTROY`의 락 획득 순서를
  정적(assert 순서) 또는 동시 호출 스트레스로 검증. 현재는 중첩이 없어 교착 없음.
- **Platform-specific — Win10 DWM:** 둥근 모서리 속성(33) 호출이 무시되고 창이 정상
  생성되는지 스모크 (현재 반환값 무시 — 실기 1회 확인).

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Good | 디바운스 상태머신·서명 검증 정확, 79/79 테스트 통과 |
| Runtime Stability | Acceptable | GDI 누수 해소·틱 가드 추가. 응답없음 근본 원인은 잔여 |
| Data Integrity | Good | 원자 저장·`.bak` 폴백 + 저장 실패 알림 + 키별 통계 영속 |
| Error Resilience | Acceptable | 롤백 재시도·서명 검증·스테이징 정리가 검증됨. 만료 타임스탬프는 fail-closed로 강화 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도. 비Windows 빌드 불가(`windows` 전용 import)가 의도와 일치 |
| Test Confidence | Acceptable | 79/79 green. 행위 변경마다 단위 테스트 추가됨 |

**먼저 수정할 3개:** ISSUE-001 업데이터 가드 실패 규명 → ISSUE-002 GDI 브러시 캐싱 →
ISSUE-003 응답없음 추적(틱 시간 측정). 1·2번은 국소적이고 다음 릴리스 전에 들어가야
업데이트 배포 신뢰도와 장시간 상주 신뢰도가 완성된다.

## 10. Addendum — 감사 제안 적용 내역 (2026-10-17)

본문 §1~§9는 감사 시점의 스냅샷이며, 아래는 그 제안을 코드에 적용한 내역이다.
`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo check --workspace --all-targets`, `cargo test --workspace` 모두 통과
(79개 전부 green: 본체 59 + 스모크 2 + 버전 동기화 6 + 헬퍼 lib 2 + 헬퍼 통합 10).

- ISSUE-001: 실패 assert에 `mode`·`result`를 출력하도록 강화했더니 실제 값이
  `Err(RollbackFailed("app exited before readiness: exit code: 17; ... (os error 5)"))`로
  확인됐다. 판정 경로는 정상이고 롤백 rename이 일시적 Access Denied로 실패한 것이다.
  `rollback_swap` 추출 + 4회·150ms 재시도 + 침묵 실패 3곳에 `warn!` 로그를 넣었다.
  `rollback_swap` 단위 테스트 2건을 추가했고, 동일 통합 테스트가 1.50초 만에 통과한다.
- ISSUE-002: `WM_CTLCOLOR*` 13곳을 색상별 캐시(`paint::cached_brush`)로 교체하고
  `apply_window_chrome`(기동·`WM_SETTINGCHANGE`)에서 캐시를 비운다.
  동일 색상은 동일 핸들임을 assert하는 단위 테스트를 추가했다.
- ISSUE-003: `update_stats`에 재진입 가드(`StatsTickGuard`, 중첩 틱 드롭)와
  250ms 초과 틱 `warn!` 측정을 넣고, `bootstrap::run`에 tracing subscriber를 초기화했다.
  가드 단위 테스트 + 1만 회 render·diff 시간 상한 테스트를 추가했다. 근본 원인은 잔여.
- ISSUE-004: `config::save() -> bool`로 바꾸고, `on_save` 실패 시 한/영 메시지박스로
  알린다. 기존 왕복 테스트는 `assert!(save(..))`로 강화하고, 저장 불가 조건
  (`APPDATA`가 파일인 경우) 테스트를 추가했다.
- Gaps: 키별 누적 통계(`stat.XX=presses,blocked`, 0이 아닌 것만)를 영속화하고
  기동·저장·종료 경로에 live 값을 합산하도록 연결했다(왕복 테스트 포함).
  불리언 파싱을 `!= "0"`으로 통일하고(비정상값 테스트 포함),
  `expires_at` 파싱 실패를 fail-closed로 바꿨다(서명 없는 합성 페이로드로 직접 테스트).
  `min_chatter` 경계(`==`는 글리치 제외) 테스트를 추가했다.
- Docs: `README` 테스트 수를 양 언어 모두 79개로 정정하고,
  `CLAUDE.md`·`GEMINI.md` 게이트를 실제 pre-push(`fmt + clippy + check`)와 일치시켰다.
  `AGENTS.md`는 여전히 없으며, 에이전트 규칙은 두 문서가 대신한다.
