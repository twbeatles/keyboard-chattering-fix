# CLAUDE.md — KeyboardChatteringFix (`kcfix-rs`)

Windows 저수준 키보드 훅(`WH_KEYBOARD_LL`) 기반 채터링 방지 상주 필터. 단일 네이티브 바이너리 + `kcfix-updater` 헬퍼(워크스페이스). 응답은 한국어로.

## 모듈 지도

- `src/main.rs` — 진입점만 (`app::cli` → `app::bootstrap::run`)
- `src/app/` — `bootstrap`(기동 순서), `cli`(`--version`/`--check-update`), `instance`(단일 인스턴스 뮤텍스), `state`(`SHARED`/`START`/`HOOK`)
- `src/input/` — `hook`(전용 훅 스레드. 앱은 `start`/`stop`/`restart`(워치독)로 관리, 테스트는 `spawn_hook_thread`. 훅에서 할당·파일 I/O 금지. 마우스 버튼 필터는 같은 스레드의 `WH_MOUSE_LL`을 켜 둔 동안만 설치 — `set_mouse_filter`가 스레드 메시지로 설치/제거, 버튼은 `VK_LBUTTON` 등으로 매핑해 같은 `Debouncer` 사용, 이동 이벤트는 락 전에 통과. 떼기 확인(`iReleaseDeferMs`): 뗌을 `Decision::Defer`로 보류 → 훅 스레드 타이머가 `due_releases`로 만료분을 큐에 넣고, 별도 injector 스레드가 `SendInput`(표식 `INJECT_MARK`, 훅은 표식 이벤트를 그대로 통과). 워치독 프로브(`PROBE_VK` 표식 키업)는 훅이 삼키고 `PROBES_SEEN` 증가), `sound`(캐시된 WAV 재생)
- `src/config/` — `model`(`Settings`), `storage`(원자적 INI 저장: tmp `sync_all` → 유효한 본 파일만 `.bak` 복사 → `MoveFileExW(WRITE_THROUGH)`. 읽기: UTF-8 BOM 제거·비UTF-8은 시스템 코드 페이지로 해석, 헤더·알려진 키가 없는 본 파일(전원 차단 후 NUL 등)은 손상으로 보고 `.bak` 폴백. 값의 줄 끝 주석은 "공백 + `#`/`;`"부터 무시(`lpSoundFile`은 제외, `sAutoDisableApps`의 `;`는 구분자). README 샘플을 `include_str!`로 파싱하는 테스트가 문서와 파서를 묶음), `autostart`(Run 인용 경로·작업 관리자 `StartupApproved` 존중, 관리자 로그인 작업은 Task Scheduler COM으로 등록(임시 파일·`schtasks` 없음, 사용자=토큰 SID, 다른 관리자 계정 승격이면 `TaskError::OtherAccount`), 작업 등록 표식 `HKCU\Software\KeyboardChatteringFix\ElevatedTask`), `sound`(`ValidatedSound`: 경로 검증 + UTF-16 사전 인코딩)
- `src/debounce.rs` — 시간 기반 필터 코어 (O(1)/이벤트, 떼기 확인 만료 검사만 512칸 순회). `decide_event`가 `Decision`(Pass/Block/Defer/Flush) 반환. 타이밍은 물리 키 슬롯(`slot_of`: vk + 확장 플래그, 512칸), 통계·임계값은 vk(256). 차단된 Down 뒤에는 `held`를 세워 다음 오토리핏을 통과시킴(누르는 중 바운스), `VK_PACKET`/`VK_PROCESSKEY`는 항상 통과·미집계, 청소 모드에서 Up은 통과, 키별 간격 히스토그램(`gap_histogram`, 생성 시 1회 할당·훅은 증가만)
- `src/recommend.rs` — 히스토그램 → 키별 권장 기준(`Advice`). 순수 함수, 단위 테스트
- `src/i18n/` — `language`(`Language::ALL` 레지스트리, `LanguagePref`=자동/고정, `system_language()` Windows 언어 감지) + `strings`(모든 가시 문자열은 **지원 언어 전부**에 존재). 새 언어 추가 절차는 `language.rs` 상단 주석: 변형 추가 → `ALL`/`code`/`native_name` → `strings.rs` 표 + `strings()` 분기 → `updater/error.rs` 분기(컴파일러가 빠뜨린 곳을 알려줌). `strings.rs` 테스트가 모든 언어의 빈 문구·플레이스홀더·번역 누락을 영어 기준으로 검사
- `src/ui/` — `layout`(선언적 컨트롤 표: 페이지·논리 좌표·텍스트 바인딩, DPI 스케일 `sc()`; 생성/번역/색/페이지 전환이 모두 이 표를 순회), `window`(메시지 디스패치·페인트·페이지 전환), `widgets`(오너드로 토글/버튼/탭/상태 알약), `stats`(홈 통계: 스냅샷→렌더→diff), `history`(기록 탭: 순수 렌더 + 가상 ListView, 리스트 공용 헬퍼 `setup_list`/`set_columns`/`write_cell`), `keys_page`(키·마우스 탭: 마우스 스위치·키별 기준·권장값. 변경은 `settings_form::set_key_override`/`set_mouse_filter`로 즉시 적용·저장), `keys`(VK 이름·키 필터·선택 목록), `foreground`(자동 일시 중지·관리자 창 감지: 전경 창이 바뀔 때만 프로세스 조회, 훅은 `Shared.suspended`만 읽음), `hotkeys`(단축키 파싱·등록), `watchdog`(훅 감시: 사용자 활동 중 훅 무소식이면 프로브 → 실패 시 `hook::restart`. 관리자 창이 전경이고 우리가 비관리자면 검사 생략 — 그 창 입력은 훅에 안 오고 프로브는 UIPI가 조용히 버림), `elevation`(관리자 권한 재실행·작업 스케줄러. 작업 변경은 워커 스레드 → `WM_ELEVATED_TASK_DONE`, 관리자 기동 시 `refresh_elevated_task`로 표식 동기화·옮겨진 exe 재등록. 작업을 켜도 Run은 지우지 않음(대비책). exe가 Program Files·Windows 밖이면 켜기 전 경고(`exe_in_protected_location`, 기본 No)), `files`(CSV·설정 내보내기/가져오기), `commands`(메뉴바·트레이·버튼 공용 동작), `menu`(메뉴 바), `tray`, `settings_form`(폼 로드·변경 표시·저장), `update`(워커 스레드. `update_route`: 패키지 관리자 설치본 → 명령 안내, 관리자 권한 실행 중 → 인앱 업데이트 거부·일반 실행 안내, 그 외 인앱), `paint`(테마 캐시·폰트·컨트롤 테마), `text`, `controls`, `diag`(콘솔 없는 GUI용 파일 진단 로그), `dialog`, `ids`, `state`
- `src/updater/` + `crates/kcfix-updater/` — Ed25519 서명 매니페스트, 핀 고정 URL, 이중 SHA-256 검증, 헬퍼 교체/롤백. 프록시: `HTTPS_PROXY`/`ALL_PROXY`(ureq 기본), 없으면 Windows 수동 프록시(`http::system_proxy`, PAC 미지원). 오류는 `UpdateErrorKind`(한/영 문구 내장)로 표현하고 UI는 `err.message(lang)`로 표시

## 핫패스 규칙 (성능)

- 훅(`hook_proc`/`mouse_proc`): 메시지 펌프만 도는 **전용 스레드**에서 실행(UI 스레드가 멈춰도 입력 지연·훅 제거 없음). 락 + `decide_event`만. 할당·파일 I/O 금지, Win32 호출은 차단 시 사운드(`PlaySoundW`/`MessageBeep`, `Shared.sound_file.wide` 캐시)와 떼기 확인용 `SetTimer`(스레드 타이머, 1개만)·`PostThreadMessageW`(자기 스레드)만 허용. `SendInput`은 훅 스레드에서 부르지 말 것(injector 스레드 사용). `SHARED` 락은 이제 실제로 스레드 간 경합하므로 UI 쪽 락 구간을 짧게 유지.
- 500ms 타이머(`update_stats`): 스냅샷 → `render_stats` → `diff_stats` → 변경분만 `SetWindowTextW`. 락을 쥔 채 Win32 호출 금지.
- 페인트(`WM_PAINT`/`WM_CTLCOLOR*`): `theme_is_dark()` 캐시 사용. 레지스트리 읽기는 `refresh_theme()`에서만 (기동 + `WM_SETTINGCHANGE`).
- 새 렌더/diff 로직은 반드시 단위 테스트(`stats.rs`의 `render_*`/`diff_*`, `history.rs`의 `render_*`)로 검증. HWND 없이 테스트 가능해야 함.
- 기록 탭은 기록 탭이 보일 때만, `log_seq`가 바뀐 틱에만 다시 렌더. 리스트는 `LVS_OWNERDATA`라 행 문자열은 `LVN_GETDISPINFOW`에서 꺼내 씀.
- 로그 버퍼 용량은 `Debouncer::set_log_limit`(UI 스레드)에서 미리 예약 → 훅의 `push_log`는 재할당하지 않음. `log_limit` 필드를 직접 쓰지 말 것.
- 사운드 경로 검증(`resolve_sound_file`, 디스크 I/O)은 `SHARED` 락 밖, 워커 스레드에서(`apply_sound_async`, 세대 번호로 늦게 끝난 검증 무시). `ShellExecuteW`도 워커 스레드(COM 초기화 포함).
- 설정 쓰기는 `persist_with` 계열(`persist_live`: 종료·세션 종료·5분 자동 저장·통계 초기화·키별 통계 지우기·언어 변경·설정 내보내기 / `set_key_override`·`set_mouse_filter`: 키·마우스 탭)과 `on_save`뿐. 모두 디스크의 파일 전용 필드(`key.XX`)를 `config::try_load()`로 다시 읽어 병합하므로 실행 중 파일 편집이 보존되고, 파일을 읽을 수 없으면 메모리(`SAVED`) 값을 유지(`on_save`는 `save_base`). `config::load()`(실패 시 기본값)로 병합 기준을 잡지 말 것. 예외: 가져오기(`files::import_settings`)는 직접 저장 1회 후 `on_save`로 확정한다.
- 새 컨트롤 추가 = `layout::SPECS` 한 줄 + (필요 시) `ids.rs`. `layout` 테스트가 ID 중복·겹침·카드 이탈·빈 문구를 잡음.

## 게이트 (푸시 전 자동 실행됨: `.githooks/pre-push`)

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
# 전체 테스트 스위트는 CI(windows-ci)에서 실행
```

릴리스 전 수동 E2E(실제 바이너리·GUI 기동, `cargo test` 밖. GitHub Actions의 `e2e` 워크플로로도 실행 가능): `scripts/e2e/README.md` — 훅·UI 정지·세션 종료 저장(`hook_session_e2e.ps1`), 떼기 확인·마우스·단축키·자동 일시 중지(`features_e2e.ps1`), 헬퍼 연속 교체(`update_swap_e2e.ps1`, 게시된 구 헬퍼는 `-HelperExe`). 릴리스 후 `bucket/`·`packaging/winget/` 해시 갱신(`docs/UPDATE.md`). 배포본의 헬퍼 상태는 `KeyboardChatteringFix.exe --check-helper`.

## 철칙

- `config::VERSION` ↔ `Cargo.toml` `[package] version` ↔ `v{VERSION}` 태그를 함께 올릴 것 (`tests/version_sync.rs`가 강제).
- `KCFIX_DBG_*`·`dlog` 같은 임시 진단 코드는 커밋 금지 (과거 응답없음 진단 잔재 — 완전히 제거됨).
- `pub use` 허브 재수출은 실제로 쓰이는 것만 (`unused_imports` 경고 대상).
- 테스트는 개수 채우기 금지: `CARGO_BIN_EXE_*` 스모크, 경계값·폴백 단위 테스트, 기존 약한 assertion 강화(`<= 11` → `== 10` 같은) 위주.
- 새 가시 문자열은 모든 지원 언어에 추가 (`strings.rs` 테스트가 강제).
- GUI를 여는 테스트 금지 (러너가 멈춤). `--version`/`-V` 경로만 실행 테스트.

## 알려진 주의점

- GUI 기동 직후 응답없음 리포트 이력 있음 — 타이머 `SetWindowTextW` 경로 의심. 현재 변경분(락 밖 Win32 호출·diff 갱신·틱 재진입 가드·느린 틱 로깅)이 노출 면적을 줄였으나 원인 확정 전. 훅은 전용 스레드로 분리되어 UI가 멈춰도 필터는 계속 동작함. 재발 시 `%TEMP%\kcfix-diag.log`(느린 틱·훅 재시작 기록, `ui::diag`)를 먼저 확인할 것.
- 언어 설정(`sLanguage`)은 `auto`(기본: Windows 언어 따라가기, 한국어면 한국어·그 외 영어) 또는 언어 코드. `KeyboardChatteringFix.exe --check-language`가 자동 선택 결과를 출력하고, 환경 변수 `KCFIX_LOCALE=de-DE`로 다른 PC의 언어를 재현·테스트할 수 있음. v0.2.1 이하가 파일에 저장해 둔 `ko`/`en`은 고정 언어로 유지됨.
- 단일 인스턴스 테스트는 고유 뮤텍스 이름을 써서 실제 트레이 앱 옆에서도 통과함. `hook_runs_on_a_dedicated_thread_and_stops`는 테스트 중 실제 LL 훅을 잠깐 설치함(전부 통과시킴, 데스크톱 없는 세션에서는 건너뜀).
- 키별 임계값(`key.XX`)은 [키·마우스] 탭에서 바꾸면 즉시 적용·파일 기록(디스크 값을 다시 읽어 그 키만 바꿈). 파일을 직접 편집한 경우 **Save**를 누르면 즉시 적용, 아니면 재시작 시 적용. 파일이 per-key 값의 원본이라는 전제(`adopt_file_only_fields`)를 깨지 말 것.
- 업데이트 헬퍼는 인앱 업데이트로 교체되지 않음 → 헬퍼 CLI는 추가만 가능, 새 정보는 환경 변수로 전달(`docs/UPDATE.md` "헬퍼 호환성 정책").
- 메인 창은 `WS_CLIPCHILDREN` 필수 — 빠지면 더블버퍼 `BitBlt`가 자식 컨트롤을 덮어 일부 컨트롤이 안 보임.
- comctl6 매니페스트 + 시스템 DPI 인식(`build.rs`). 좌표는 전부 96-DPI 논리값으로 쓰고 `sc()`로 변환.
- GUI 수동 검증 시 `APPDATA`를 임시 폴더로 바꿔 실행할 것(실제 설정 파일 보호). 단, `HKCU\...\Run`(자동 실행)과 작업 스케줄러는 공유됨. **저장**은 `sync_autostart`로 토글 상태가 실제와 다르거나 등록된 exe가 사라졌을 때만 `Run`을 다시 쓰지만, 테스트 빌드에서 자동 실행 토글을 바꿔 저장하면 실제 항목이 테스트 exe로 바뀜. 자동 실행 "켜짐"은 `Run` 값 존재 + 작업 관리자 시작 프로그램 탭에서 미사용 표시(`StartupApproved\Run` 첫 바이트 홀수)가 없을 때이며, 켜기는 그 표시를 지움. 합성 키 입력(`keybd_event`)은 다른 창이 전경일 때 OS가 버릴 수 있어 훅 테스트로 부적합.
- **Raw input(`RegisterRawInputDevices`) 등록 금지**: 키보드 raw input을 등록하면 앱 창이 전경일 때 LL 훅이 입력을 받지 못함(v0.3.0 개발 중 실측, `features_e2e`/창 표시 상태 프로브로 발견). 워치독이 raw input 대신 프로브를 쓰는 이유.
- 단일 인스턴스 뮤텍스를 관리자 인스턴스가 쥐고 있으면 일반 권한의 `CreateMutexW`/`OpenMutexW`는 `ACCESS_DENIED` → "이미 실행 중"으로 취급해야 함(fail open 금지). 로그인 시 일반 `--autostart` 인스턴스는 표식이 있으면 관리자 인스턴스를 `ELEVATED_START_GRACE`(20초)까지 기다린 뒤 대체 실행하고 표식을 지움(관리자 작업이 늦게 떠서 조용히 종료할 때 표식을 다시 세움). `--autostart`는 항상 트레이로 시작, `--after-pid` 대기는 30초(`app::bootstrap`).
- 떼기 확인 재주입은 injector 스레드의 `send_retrying`이 `SendInput` 거부(보안 데스크톱) 시 100ms 간격 최대 30회 재시도. 종료 시 훅 스레드의 `send`는 1회만(대기 금지).
- 창 높이 690(논리). 설정·고급·키 탭 하단 카드는 저장 버튼(614)과 겹치지 않게 602 이하로.
