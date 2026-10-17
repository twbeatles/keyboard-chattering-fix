# CLAUDE.md — KeyboardChatteringFix (`kcfix-rs`)

Windows 저수준 키보드 훅(`WH_KEYBOARD_LL`) 기반 채터링 방지 상주 필터. 단일 네이티브 바이너리 + `kcfix-updater` 헬퍼(워크스페이스). 응답은 한국어로.

## 모듈 지도

- `src/main.rs` — 진입점만 (`app::cli` → `app::bootstrap::run`)
- `src/app/` — `bootstrap`(기동 순서), `cli`(`--version`/`--check-update`), `instance`(단일 인스턴스 뮤텍스), `state`(`SHARED`/`START`/`HOOK`)
- `src/input/` — `hook`(전용 훅 스레드 `spawn_hook_thread` + 키별 `decide`, 훅에서 할당·파일 I/O 금지), `sound`(캐시된 WAV 재생)
- `src/config/` — `model`(`Settings`), `storage`(원자적 INI 저장·`.bak` 폴백), `autostart`(Run 인용 경로), `sound`(`ValidatedSound`: 경로 검증 + UTF-16 사전 인코딩)
- `src/debounce.rs` — 시간 기반 필터 코어 (O(1)/이벤트)
- `src/i18n/` — `language` + `strings` (모든 가시 문자열은 한/영 쌍으로 존재)
- `src/ui/` — `layout`(선언적 컨트롤 표: 페이지·논리 좌표·텍스트 바인딩, DPI 스케일 `sc()`; 생성/번역/색/페이지 전환이 모두 이 표를 순회), `window`(메시지 디스패치·페인트·페이지 전환), `widgets`(오너드로 토글/버튼/탭/상태 알약), `stats`(홈 통계: 스냅샷→렌더→diff), `history`(기록 탭: 순수 렌더 + 가상 ListView), `keys`(VK 이름·키 필터), `commands`(메뉴바·트레이·버튼 공용 동작), `menu`(메뉴 바), `tray`, `settings_form`(폼 로드·변경 표시·저장), `update`(워커 스레드), `paint`(테마 캐시·폰트·컨트롤 테마), `text`, `controls`, `dialog`, `ids`, `state`
- `src/updater/` + `crates/kcfix-updater/` — Ed25519 서명 매니페스트, 핀 고정 URL, 이중 SHA-256 검증, 헬퍼 교체/롤백. 오류는 `UpdateErrorKind`(한/영 문구 내장)로 표현하고 UI는 `err.message(lang)`로 표시

## 핫패스 규칙 (성능)

- 훅(`hook_proc`): 메시지 펌프만 도는 **전용 스레드**에서 실행(UI 스레드가 멈춰도 입력 지연·훅 제거 없음). 락 + `decide`만. 할당·파일 I/O 금지, Win32 호출은 차단 시 사운드(`PlaySoundW`/`MessageBeep`, `Shared.sound_file.wide` 캐시)만 허용. `SHARED` 락은 이제 실제로 스레드 간 경합하므로 UI 쪽 락 구간을 짧게 유지.
- 500ms 타이머(`update_stats`): 스냅샷 → `render_stats` → `diff_stats` → 변경분만 `SetWindowTextW`. 락을 쥔 채 Win32 호출 금지.
- 페인트(`WM_PAINT`/`WM_CTLCOLOR*`): `theme_is_dark()` 캐시 사용. 레지스트리 읽기는 `refresh_theme()`에서만 (기동 + `WM_SETTINGCHANGE`).
- 새 렌더/diff 로직은 반드시 단위 테스트(`stats.rs`의 `render_*`/`diff_*`, `history.rs`의 `render_*`)로 검증. HWND 없이 테스트 가능해야 함.
- 기록 탭은 기록 탭이 보일 때만, `log_seq`가 바뀐 틱에만 다시 렌더. 리스트는 `LVS_OWNERDATA`라 행 문자열은 `LVN_GETDISPINFOW`에서 꺼내 씀.
- 로그 버퍼 용량은 `Debouncer::set_log_limit`(UI 스레드)에서 미리 예약 → 훅의 `push_log`는 재할당하지 않음. `log_limit` 필드를 직접 쓰지 말 것.
- 사운드 경로 검증(`resolve_sound_file`, 디스크 I/O)은 `SHARED` 락 밖, 워커 스레드에서(`apply_sound_async`, 세대 번호로 늦게 끝난 검증 무시). `ShellExecuteW`도 워커 스레드(COM 초기화 포함).
- 설정 쓰기는 `persist_live`(종료·세션 종료·5분 자동 저장·통계 초기화·언어 변경)와 `on_save`뿐. 둘 다 디스크의 파일 전용 필드(`key.XX`)를 다시 읽어 병합(`Settings::adopt_file_only_fields`)하므로 실행 중 파일 편집이 보존됨.
- 새 컨트롤 추가 = `layout::SPECS` 한 줄 + (필요 시) `ids.rs`. `layout` 테스트가 ID 중복·겹침·카드 이탈·빈 문구를 잡음.

## 게이트 (푸시 전 자동 실행됨: `.githooks/pre-push`)

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
# 전체 테스트 스위트는 CI(windows-ci)에서 실행
```

릴리스 전 수동 E2E(실제 바이너리·GUI 기동, `cargo test` 밖. GitHub Actions의 `e2e` 워크플로로도 실행 가능): `scripts/e2e/README.md` — 훅·UI 정지·세션 종료 저장(`hook_session_e2e.ps1`), 헬퍼 연속 교체(`update_swap_e2e.ps1`, 게시된 구 헬퍼는 `-HelperExe`). 배포본의 헬퍼 상태는 `KeyboardChatteringFix.exe --check-helper`.

## 철칙

- `config::VERSION` ↔ `Cargo.toml` `[package] version` ↔ `v{VERSION}` 태그를 함께 올릴 것 (`tests/version_sync.rs`가 강제).
- `KCFIX_DBG_*`·`dlog` 같은 임시 진단 코드는 커밋 금지 (과거 응답없음 진단 잔재 — 완전히 제거됨).
- `pub use` 허브 재수출은 실제로 쓰이는 것만 (`unused_imports` 경고 대상).
- 테스트는 개수 채우기 금지: `CARGO_BIN_EXE_*` 스모크, 경계값·폴백 단위 테스트, 기존 약한 assertion 강화(`<= 11` → `== 10` 같은) 위주.
- GUI를 여는 테스트 금지 (러너가 멈춤). `--version`/`-V` 경로만 실행 테스트.

## 알려진 주의점

- GUI 기동 직후 응답없음 리포트 이력 있음 — 타이머 `SetWindowTextW` 경로 의심. 현재 변경분(락 밖 Win32 호출·diff 갱신·틱 재진입 가드·느린 틱 로깅)이 노출 면적을 줄였으나 원인 확정 전. 훅은 전용 스레드로 분리되어 UI가 멈춰도 필터는 계속 동작함.
- 단일 인스턴스 테스트는 고유 뮤텍스 이름을 써서 실제 트레이 앱 옆에서도 통과함. `hook_runs_on_a_dedicated_thread_and_stops`는 테스트 중 실제 LL 훅을 잠깐 설치함(전부 통과시킴, 데스크톱 없는 세션에서는 건너뜀).
- 키별 임계값(`key.XX`)은 설정 파일 전용, UI 없음. 파일 편집 후 **Save**를 누르면 즉시 적용, 아니면 재시작 시 적용.
- 업데이트 헬퍼는 인앱 업데이트로 교체되지 않음 → 헬퍼 CLI는 추가만 가능, 새 정보는 환경 변수로 전달(`docs/UPDATE.md` "헬퍼 호환성 정책").
- 메인 창은 `WS_CLIPCHILDREN` 필수 — 빠지면 더블버퍼 `BitBlt`가 자식 컨트롤을 덮어 일부 컨트롤이 안 보임.
- comctl6 매니페스트 + 시스템 DPI 인식(`build.rs`). 좌표는 전부 96-DPI 논리값으로 쓰고 `sc()`로 변환.
- GUI 수동 검증 시 `APPDATA`를 임시 폴더로 바꿔 실행할 것(실제 설정 파일 보호). 합성 키 입력(`keybd_event`)은 다른 창이 전경일 때 OS가 버릴 수 있어 훅 테스트로 부적합.
