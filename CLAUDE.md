# CLAUDE.md — KeyboardChatteringFix (`kcfix-rs`)

Windows 저수준 키보드 훅(`WH_KEYBOARD_LL`) 기반 채터링 방지 상주 필터. 단일 네이티브 바이너리 + `kcfix-updater` 헬퍼(워크스페이스). 응답은 한국어로.

## 모듈 지도

- `src/main.rs` — 진입점만 (`app::cli` → `app::bootstrap::run`)
- `src/app/` — `bootstrap`(기동 순서), `cli`(`--version`/`--check-update`), `instance`(단일 인스턴스 뮤텍스), `state`(`SHARED`/`START`/`HOOK`)
- `src/input/` — `hook`(키별 `decide`, 훅에서 할당·파일 I/O 금지), `sound`(캐시된 WAV 재생)
- `src/config/` — `model`(`Settings`), `storage`(원자적 INI 저장·`.bak` 폴백), `autostart`(Run 인용 경로), `sound`(`ValidatedSound`: 경로 검증 + UTF-16 사전 인코딩)
- `src/debounce.rs` — 시간 기반 필터 코어 (O(1)/이벤트)
- `src/i18n/` — `language` + `strings` (모든 가시 문자열은 한/영 쌍으로 존재)
- `src/ui/` — `window`(메시지 디스패치), `stats`(스냅샷→렌더→diff; 타이머 틱에 바뀐 컨트롤만 갱신), `text`, `settings_form`(숫자 파싱 헬퍼), `tray`, `update`(워커 스레드), `paint`(테마 캐시), `controls`, `dialog`, `ids`, `state`
- `src/updater/` + `crates/kcfix-updater/` — Ed25519 서명 매니페스트, 핀 고정 URL, 이중 SHA-256 검증, 헬퍼 교체/롤백

## 핫패스 규칙 (성능)

- 훅(`hook_proc`): 락 + `decide`만. 할당·파일 I/O·Win32 호출 금지. 사운드는 `Shared.sound_file.wide` 캐시 재생.
- 500ms 타이머(`update_stats`): 스냅샷 → `render_stats` → `diff_stats` → 변경분만 `SetWindowTextW`. 락을 쥔 채 Win32 호출 금지.
- 페인트(`WM_PAINT`/`WM_CTLCOLOR*`): `theme_is_dark()` 캐시 사용. 레지스트리 읽기는 `refresh_theme()`에서만 (기동 + `WM_SETTINGCHANGE`).
- 새 렌더/diff 로직은 반드시 `stats.rs` 단위 테스트(`render_*`, `diff_*`)로 검증. HWND 없이 테스트 가능해야 함.

## 게이트 (푸시 전 자동 실행됨: `.githooks/pre-push`)

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
# 전체 테스트 스위트는 CI(windows-ci)에서 실행
```

## 철칙

- `config::VERSION` ↔ `Cargo.toml` `[package] version` ↔ `v{VERSION}` 태그를 함께 올릴 것 (`tests/version_sync.rs`가 강제).
- `KCFIX_DBG_*`·`dlog` 같은 임시 진단 코드는 커밋 금지 (과거 응답없음 진단 잔재 — 완전히 제거됨).
- `pub use` 허브 재수출은 실제로 쓰이는 것만 (`unused_imports` 경고 대상).
- 테스트는 개수 채우기 금지: `CARGO_BIN_EXE_*` 스모크, 경계값·폴백 단위 테스트, 기존 약한 assertion 강화(`<= 11` → `== 10` 같은) 위주.
- GUI를 여는 테스트 금지 (러너가 멈춤). `--version`/`-V` 경로만 실행 테스트.

## 알려진 주의점

- GUI 기동 직후 응답없음 리포트 이력 있음 — 타이머 `SetWindowTextW` 경로 의심. 현재 변경분(락 밖 Win32 호출·diff 갱신·틱 재진입 가드·느린 틱 로깅)이 노출 면적을 줄였으나 원인 확정 전.
- `second_acquisition_fails_while_first_held` 테스트는 실제 트레이 앱 옆에서 실패 — CI에서만 실행.
- 키별 임계값(`key.XX`)은 설정 파일 전용, UI 없음.
- `LogEvent` 버퍼는 기록만 되고 화면에 미표시 (`#[allow(dead_code)]`에 사유 명시).
