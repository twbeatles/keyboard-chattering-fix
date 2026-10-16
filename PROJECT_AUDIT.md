# Project Audit — KeyboardChatteringFix (`kcfix-rs`)

- 대상: `D:\twbeatles-repos\kcfix-rs` (단일 Rust 바이너리 + `kcfix-updater` 헬퍼 워크스페이스)
- 감사일: 2026-10-16, 코드 수정 없이 읽기·테스트 실행만으로 감사
- 감사 관점: 기능 구현 정확성 · 런타임 안정성 (스타일·취향 제외)

## 1. Executive Summary

전체 상태: 핵심 필터 로직은 정확하고 단위 테스트로 잘 방어되어 있으나,
프로세스 수준의 안정 장치(중복 실행 방지, 원자적 설정 저장)가 빠져 있고
저수준 훅 콜백 안의 파일 I/O가 조건부로 보호 기능을 침묵 중단시킬 수 있다.
전체 위험도: **Needs Work** (데이터 파괴급 문제는 없음).

가장 중요한 문제 4개:

1. [ISSUE-001] 중복 실행 방지 없음 — 두 프로세스가 각자 훅을 걸고 입력을 중복 차단 (High / Confirmed)
2. [ISSUE-002] 시작프로그램 레지스트리 경로 미인용 — 공백 포함 설치 경로에서 부팅 자동실행 실패 (Medium / Confirmed)
3. [ISSUE-003] 훅 콜백 안에서 커스텀 WAV 존재 확인(`Path::exists`) — 느린 경로에서 전역 입력 정지 후 훅 침묵 해제 가능 (Medium / Likely)
4. [ISSUE-004] `settings.ini` 비원자적 저장 — 충돌 시 설정 전체가 기본값으로 침묵 초기화 (Medium / Likely)

데이터 손상/유실 가능성: 영구 데이터 파괴는 없음. 설정값 초기화(ISSUE-004)와
복수 인스턴스 간 카운터/설정 덮어쓰기(ISSUE-001)가 현실적인 유실 범위다.

가장 먼저 수정해야 할 영역: `main.rs` 진입점(단일 인스턴스 뮤텍스) →
`config.rs` 자동실행 경로 인용 → 훅 콜백의 사운드 경로 캐싱.

## 2. Project Understanding

### 목적

기계식 키보드 스위치 바운스(채터링·이중 입력)를 Windows 저수준 키보드 훅
(`WH_KEYBOARD_LL`)으로 가로채 드롭하는 상주형 필터. 관리자 권한·DLL 인젝션
불필요. 트레이 상주, Win32 네이티브 UI, Ed25519 서명 기반 자동 업데이트.

### 주요 entrypoint

- `src/main.rs::main` — CLI(`--version`/`--check-update`) → 설정 로드 →
  전역 상태 초기화 → 1ms 타이머 → 창 생성 → 훅 설치 → 메시지 펌프
- `src/main.rs::hook_proc` — 실제 키보드 필터 콜백 (OS가 호출)
- `crates/kcfix-updater/src/main.rs::main` — 업데이트 교체 전용 헬퍼 프로세스
- `scripts/build_update_manifest.py` — 릴리스 시 서명 매니페스트 생성
- `scripts/build_release.ps1` — 릴리스 빌드·스모크·패키징

### 핵심 모듈

| 모듈 | 역할 |
|---|---|
| `debounce.rs` | `Debouncer` 상태 머신 (임계값·홀드·통계·청소 모드·로그 버퍼) |
| `config.rs` | INI 파싱/저장, `VERSION`·공개키 상수, Run 레지스트리 |
| `ui.rs` | Win32 창·트레이 메뉴·대화상자·업데이트 확인 흐름 |
| `i18n.rs` | 한/영 문자열 테이블 |
| `updater/` | 서명 검증·핀 고정 URL·다운로드 검증·스테이징·헬퍼 실행 |
| `kcfix-updater` | 프로세스 종료 대기·해시 재검증·교체·롤백·재실행 검증 |

### 데이터 저장 방식

- `%APPDATA%\KeyboardChatteringFix\settings.ini` (구 `KcfixRs` 경로 읽기 호환):
  임계값·플래그·언어·키별 임계값·누적 카운터. 세션 텔레메트리(키별 통계·로그)는
  메모리 전용(`VecDeque`, `log_limit` 상한)이며 영속화하지 않음.
- 레지스트리 `HKCU\...\Run\KeyboardChatteringFix` (자동 실행, REG_SZ).
- 업데이트 스테이징: `%TEMP%` 고유 경로 (프로세스ID·시간·시퀀스).

### 외부 의존성

- `windows 0.58` (Win32 API), `ureq 3` (HTTPS), `ed25519-dalek 3` + `base64` +
  `sha2`/`hex` (업데이트 검증), `clap 4` (헬퍼 CLI), `winresource` (버전 리소스).
- 네트워크: `github.com` 릴리스에서 `update.json`·EXE 수신 (타임아웃
  접속 30초/수신 60초/전체 90초). 재시도 없음 (수동 재확인 흐름이므로 허용 가능).

### 핵심 실행 흐름

`키 입력 → hook_proc → (청소 모드 | 합성입력 제외 | debounce.decide) →
차단/통과 → 통계·사운드 → 결과`

`설정 저장 → on_save → 레지스트리+메모리+INI → 적용`

`업데이트 확인 → update.json 수신 → 서명·URL·버전 검증 → %TEMP% 다운로드 검증 →
헬퍼 실행 → 앱 종료 → 교체·재실행 검증 → (실패 시 롤백·이전 버전 실행)`

## 3. Audit Coverage & Limitations

실제 확인한 모듈: `main.rs`·`debounce.rs`·`config.rs`·`i18n.rs`(문자열 키)·
`ui.rs`(트레이·저장·종료·업데이트 흐름부)·`updater/` 7파일·헬퍼 `lib.rs`·`main.rs`·
`scripts` 3종·`release.yml`·`windows-ci.yml`·`version_sync.rs`·`README.md`·
`docs/UPDATE.md` 전문.

CodeGraph(`codegraph explore`)로 분석한 호출 관계: 진입점·훅 흐름,
트레이/업데이트 흐름(`show_tray_menu`→`spawn_update_check`→`staging`,
호출자 1~2곳·테스트 0), 설정(`set_autostart` 호출자 `ui.rs` 1곳),
`UpdateError`·`UpdateManifest` 영향 범위. 단, 감사 시점 인덱스가
`ui.rs`·`config.rs`의 최신 디스크 상태를 반영하지 못해 해당 파일은 직접
전수 열람으로 보완했고, CodeGraph 결과는 호출 존재 여부 확인용으로만 사용했다.

실행한 테스트: `cargo test --workspace` (exit 0). 헬퍼 10 + 앱 23 +
`version_sync` 3 = 36개 전부 통과. GUI 실행·실제 네트워크 업데이트 E2E·
릴리스 워크플로는 실행하지 않았다.

확인하지 못한 환경: 다중 Windows 버전(10/11)·저사양 PC의 훅 지연,
실제 GitHub Releases 게시 후의 `latest/download` 리다이렉트 동작,
메모리 5MB 주장의 실측, 네트워크 드라이브 WAV 경로에서의 훅 타임아웃 재현.

한계: 정적 분석+단위 테스트 기반이므로 타이밍·OS 스케줄러 의존 문제는
재현 없이 코드 근거로만 판단했으며, 해당 항목은 신뢰도를 Likely로 표기했다.

## 4. High-Risk Issues

### [ISSUE-001] 중복 실행 방지(단일 인스턴스 뮤텍스) 없음

- **위치:** `src/main.rs::main` (프로세스 시작 경로 전체)
- **우선순위:** High
- **신뢰도:** Confirmed
- **문제:** 실행 파일 더블클릭·자동실행+수동실행 등으로 프로세스가 2개 뜨면
  각자 `WH_KEYBOARD_LL` 훅을 설치한다. OS는 두 콜백을 모두 호출하고 어느
  한쪽이라도 차단하면 키가 드롭되므로, 정상 키까지 더 자주 차단되는
  과잉 필터 + 두 프로세스의 카운터 발산 + `settings.ini` 최후 기록자 승리
  + 트레이 아이콘 2개 + 업데이트 흐름 2중 실행이 발생한다.
- **발생 조건:** 동일 EXE를 2회 실행 (현실적: 더블클릭 실수, 부팅 자동실행 중
  수동 실행).
- **영향:** 핵심 기능(정상 입력 전달) 실패, 통계 불일치, 설정 덮어쓰기.
- **근거:** 코드베이스에 `CreateMutex`/이름 있는 뮤텍스·"already running" 처리
  전무(grep 확인). `SHARED`·`HOOK`는 프로세스별 정적 변수라 프로세스 간
  조율이 불가능하다.
- **반증 확인:** 상위 caller·프레임워크 보호 없음. 참조 프로젝트의
  `kakao-win32::single_instance` 같은 장치가 이 저장소에는 이식되지 않았다.
  (참조 저장소에도 동일 모듈이 존재하므로 누락이 확실하다.)
- **호출/영향 범위:** `main`→`SetWindowsHookExW`→`hook_proc`, `config::save`
  (양 프로세스), 트레이·업데이터 전역. CodeGraph상 `HOOK`·`hook_proc` 호출자
  각 1곳으로 보이나 이는 프로세스 내 기준이며, 문제는 프로세스 밖에 있다.
- **권장 수정 방향:** 시작 시 `CreateMutexW` 이름 있는 뮤텍스 선점, 선점 실패 시
  기존 창 활성화(선택) 후 종료 코드 0으로 종료. `--check-update` 같은
  1회성 CLI는 뮤텍스 대상에서 제외.
- **필요한 회귀 테스트:** 통합 테스트에서 뮤텍스 선점 후 두 번째 인스턴스
  진입 함수가 종료 경로를 타는지 확인 (CI Windows 러너에서 실행).

### [ISSUE-002] 자동실행 레지스트리 값의 경로 미인용

- **위치:** `src/config.rs::set_autostart` (281~287행, `current_exe` 기록부)
- **우선순위:** Medium
- **신뢰도:** Confirmed
- **문제:** `HKCU\...\Run`에 `C:\Program Files\...\exe` 형태를 따옴표 없이
  기록한다. 경로에 공백이 있으면 Windows가 첫 공백까지만 실행 파일로 해석해
  부팅 자동실행이 실패한다.
- **발생 조건:** 공백 포함 경로에 설치 + 트레이에서 자동실행 ON.
- **영향:** 부팅 후 보호 미동작 (사용자는 설정이 켜져 있다고 믿음).
- **근거:** 코드상 `format!("\"{exe}\"")` 인용 없음. `RegSetValueExW` 길이 계산
  (`*2`, NUL 포함)은 정확하므로 인용만 빠진 상태다.
- **반증 확인:** 호출자(`ui.rs::on_save`)·OS 차원의 자동 보정 없음.
  HKCU 자기 경로라 권한 상승형 취약점으로는 번지지 않는다.
- **호출/영향 범위:** `on_save`→`set_autostart` 단일 경로 (CodeGraph 확인).
- **권장 수정 방향:** `"\"{exe}\""` 인용 저장. 기존 미인용 값도 동일 값으로
  덮어쓰므로 마이그레이션 불필요. 인용 여부 단위 테스트 추가
  (`set` 없이 문자열 구성 분리 시) 또는 수동 검증 체크리스트에 포함.
- **필요한 회귀 테스트:** 공백 포함 가짜 경로에 대한 값 구성 테스트
  (레지스트리 실기입 없이 구성 함수 분리 후), 실기에서는 재부팅 1회 확인.

### [ISSUE-003] 훅 콜백 내 커스텀 WAV 존재 확인 I/O

- **위치:** `src/main.rs::hook_proc` (107~110행) → `play_notification_sound`
  (43~60행) → `Path::new(path).exists()`, `SHARED` 락 보유 중
- **우선순위:** Medium
- **신뢰도:** Likely (근거 강함, 네트워크 경로 재현은 못함)
- **문제:** 차단된 키마다 저수준 훅 스레드(UI 스레드와 동일)에서 파일 존재
  확인을 수행한다. 로컬 SSD에서는 수십 µs로 무해하나, `lpSoundFile`이
  네트워크·절전 디스크·분리된 드라이브를 가리키면 stat이 수백 ms 이상
  멈출 수 있다. `WH_KEYBOARD_LL` 콜백이 제한 시간(`LowLevelHooksTimeout`)을
  넘기면 Windows가 훅을 **침묵 해제**한다. 앱은 계속 돌고 트레이 배지는
  "보호 동작 중"을 표시하므로 사용자는 보호 중단을 알 수 없다.
- **발생 조건:** 커스텀 WAV 경로 설정 + 해당 경로가 느리거나 끊김.
  기본 비프음 설정에서는 `exists()`를 타지 않아 발생하지 않음(반증됨).
- **영향:** 핵심 보호 기능의 침묵 중단 (표시와 실제 상태 불일치).
- **근거:** `SND_ASYNC`는 재생만 비동기이며 `exists()`는 동기 I/O다.
  락 보유 중 수행되므로 타이머 통계 스레드도 함께 멈춘다.
- **반증 확인:** 호출 스택에 비동기 오프로딩·캐시·타임아웃 없음.
  `MessageBeep` 폴백은 파일 부재 시에만 동작하고 느린 경로에는 무력하다.
- **호출/영향 범위:** `hook_proc`(유일 생산 호출자) → 전역 입력 경로 전체.
- **권장 수정 방향:** `on_save`/시작 시점에 경로를 1회 검증·정규화해
  `Shared`에 `Option<PathBuf>`(검증됨)로 캐싱하고, 훅 안에서는 `exists()`
  재호출 없이 `PlaySoundW`만 호출 (실패 시 즉시 비프). 또는 재생을 전용
  스레드 채널로 오프로딩.
- **필요한 회귀 테스트:** 존재하지 않는·느린 경로 흉내(훅 대신
  `play_notification_sound` 직접 호출) 시 폴백 비프 경로 도달 테스트,
  `Shared`에 미검증 문자열이 보관되지 않음을 assert.

### [ISSUE-004] `settings.ini` 비원자적 저장 (충돌 시 기본값 초기화)

- **위치:** `src/config.rs::save` (210행 `fs::write`)
- **우선순위:** Medium
- **신뢰도:** Likely (메커니즘 확정, 충돌 타이밍은 저빈도)
- **문제:** `fs::write`는 파일을 자르고(truncate) 쓴다. 저장 중 프로세스
  강제종료·전원 차단 시 0바이트/깨진 파일이 남고, 다음 시작의 `load()`는
  읽기 실패·빈 맵을 **기본값으로 침묵 해석**해 사용자 설정(임계값·키별
  설정·언어)이 전부 리셋된다. 백업·저널이 없다.
- **발생 조건:** 저장 찰나(적용 클릭·종료 시 매번)의 비정상 종료.
- **영향:** 설정 유실 (필터 자체는 기본값으로 계속 동작하므로 복구 가능).
- **근거:** 표준 라이브러리 `fs::write` 동작 + `load()`의 `let Ok(...) else
  return default` 폴백. 상위 재시도·백업 없음.
- **반증 확인:** `per_key` 왕복 보존·클램프 등 정상 경로 검증은 있으나
  torn-write 보호는 전무. ISSUE-001 중복 실행 시 최후 기록자 승리로도
  카운터가 되돌아갈 수 있어 영향이 가중된다.
- **호출/영향 범위:** `on_save`·`WM_DESTROY` → 전 설정.
- **권장 수정 방향:** 임시파일 기록 후 원자적 rename + 이전 파일 `.bak`
  1세대 보관, 시작 시 본 파일 파싱 실패하면 `.bak` 폴백.
- **필요한 회귀 테스트:** 0바이트·깨진 INI를 주고 `load()`가 기본값이 아닌
  `.bak` 값을 복원하는지 통합 테스트, 왕복(round-trip) 보존 테스트.

## 5. Potential Functional Gaps

- **Confirmed Gap — `config` 왕복·클램프 테스트 없음.** `load()`의 클램프
  (1~1000)·별칭 키(`bFilterStatus`)·16진 키 파싱은 순수 함수 수준에서 테스트
  가능하나 테스트가 0개다. 회귀에 무방비.
- **Confirmed Gap — `hook_proc`·`ui.rs` 무테스트.** Win32 GUI라 단위 테스트가
  어렵다는 정당한 이유가 있으나, `spawn_update_check` 분기(최신/신버전/실패)·
  `WM_UPDATE_EXIT` 종료 경로·`on_save` 필드 매핑은 헤드리스로 분리 테스트
  가능한 부분이 혼재한다. CodeGraph에서도 관련 심볼 모두 "no tests"다.
- **추정 — 자동 업데이트 확인 없음.** 업데이트 확인이 트레이 수동 메뉴와
  `--check-update` CLI뿐이다. 버그가 아니라 현 설계 범위이며, 필요시
  시작 시 1회 백그라운드 확인이 로드맵 후순위다.
- **추정 — 매니페스트 유효기간(기본 365일) 경과 시 업데이트 정지.**
  1년 넘게 릴리스가 없으면 서명은 유효해도 `expires_at`으로 거부된다.
  다음 릴리스가 나오면 자동 해소되므로 운영상 주의 사항 수준이다.
- **추정 — 첫 릴리스 전에는 업데이트 확인이 항상 실패한다.**
  `MANIFEST_URL`이 가리키는 `latest/download/update.json`이 아직 존재하지
  않으므로, 첫 GitHub Release 게시 전까지는 "업데이트 정보 다운로드 실패"
  안내가 정상이다. 장애가 아니다.
- **추정 — INI 전용 설정(`iMinChatterMs`·`bExcludeInjected`·`iLogLimit`)은
  재시작 후 적용.** UI에 노출되지 않아 live 반영 경로가 없으며 문서에도
  재시작 필요성이 적혀 있지 않다. 기능상 오동작은 아니다.

## 6. Documentation Mismatches

- **테스트 수 표기 만료 (Low).** README 한/영 모두 "24개 테스트"라고 하나
  실제 `cargo test --workspace`는 36개(헬퍼 10 + 앱 23 + `version_sync` 3)다.
- **`LICENSE` 파일 없음 (Low).** README가 `[MIT License](LICENSE)`로 링크하나
  저장소 루트에 `LICENSE` 파일이 없고, `Cargo.toml`에도 `license` 필드가 없다.
- **"Mica Surface" 주장 과장 (Low).** README·배지는 Mica를 내세우나 구현은
  다크 타이틀바(`DWMWA_USE_IMMERSIVE_DARK_MODE`)+둥근 모서리뿐이며
  시스템 백드롭(`DWMWA_SYSTEMBACKDROP_TYPE`) 코드는 없다.
- **업데이트 사용법 누락 (Low).** `docs/UPDATE.md`는 있으나 README 본문에
  트레이 "업데이트 확인" 메뉴·`--check-update` 사용 안내가 없다
  (아키텍처 트리에서 문서 존재만 언급). 사용자가 기능을 발견하기 어렵다.
- **`CLAUDE.md`·`AGENTS.md` 없음.** 감사 지시가 확인을 요구해 명시한다:
  두 파일 모두 없으며, 그 결과 에이전트용 개발 규칙 문서가 없다.
  불일치가 아니라 부재이며, 규칙이 필요하다면 별도 작성 대상이다.

## 7. Recommended Fix Plan

### Phase 1 — Immediate

1. ISSUE-001 단일 인스턴스 뮤텍스 (`main.rs` 진입점, `--check-update` 제외).
2. ISSUE-002 Run 경로 인용 (`config.rs::set_autostart` 한 줄).
3. ISSUE-003 사운드 경로 검증 캐싱 (훅 밖으로 `exists()` 이동).

### Phase 2 — Stability

4. ISSUE-004 원자적 저장 + `.bak` 폴백 (`config.rs::save`/`load`).
5. `config` 단위 테스트 (클램프·별칭·16진 파싱·왕복 보존·깨진 파일).
6. `update.json` 만료·404 등 업데이트 실패 안내 문구 정리 (현재는 동작하나
   첫 릴리스 전 메시지가 혼란스러울 수 있음).

### Phase 3 — Structural

7. UI 로직 중 테스트 가능한 부분 분리 (`on_save` 매핑·업데이트 상태머신을
   Win32 호출과 분리).
8. `LICENSE` 파일 추가 + `Cargo.toml` 라이선스 표기, README 수치·Mica
   문구·업데이트 사용법 정정.
9. (선택) 시작 시 1회 백그라운드 업데이트 확인 + `LICENSE`·링크 점검을
   `version_sync`류 테스트로 확장.

실제 코드는 수정하지 않는다 (본 감사의 제약).

## 8. Test Recommendations

- **Unit — `config::load` 클램프:** `iThreshold=0`→1, `=5000`→1000,
  `iLogLimit=3`→10, `abc`(파싱 실패)→기본값 유지, `key.GG`(비16진) 무시,
  `key.41=200`→`per_key_ms[0x41]==Some(200)`. 기대: 모두 기본값 붕괴 없이.
- **Unit — `config` 왕복:** 모든 필드+키별 3건을 `save`→`load`해 동등 assert.
  `sLanguage` 대소문자(`EN`)·`sMeasureFrom=press` 포함.
- **Unit — `debounce` 로그 상한:** `log_limit=10`에서 50회 decide 후
  `log_events.len() <= 11` assert (현재 로직의 상한 문서화).
- **Unit — `debounce` 청소 모드:** `Up` 이벤트로 모드 유지, `Down+0x0D`로만
  해제, 해제 눌림이 앱에 전달되지 않음(현재 동작 고정).
- **Integration — 단일 인스턴스:** 뮤텍스 선점 상태에서 두 번째 진입이
  훅 설치 없이 종료 코드 0으로 끝나는지 (Windows CI 전용).
- **Integration — torn-write 복구:** 0바이트 INI + 유효 `.bak` 조합에서
  시작 설정이 `.bak` 값과 일치하는지.
- **Integration — 사운드 폴백:** 존재하지 않는 `lpSoundFile`에서 차단 발생 시
  크래시 없이 비프 경로로 빠지는지 (`play_notification_sound` 직접 호출).
- **Integration — 업데이트 거짓 서명 거부:** 버전만 바꾼 매니페스트가
  `parse_and_verify_manifest`에서 서명 오류로 거부되는지 (이미 부분 존재 —
  픽스처 변조 테스트 유지).
- **Concurrency — 업데이트 단일 비행:** 두 스레드에서 `try_begin_update`
  동시 호출 시 정확히 1곳만 성공 (기존 테스트는 순차적이므로 병렬 버전 추가).
- **E2E — 릴리스 스크립트:** `build_release.ps1 -NoSign` + `--version`
  스모크 (이미 존재 — CI에 고정).
- **Regression — 버전 드리프트:** 기존 `version_sync` 유지 + 태그 형식
  (`v`+3숫자) 검사 추가 검토.
- **Platform-specific — Win10:** 둥근 모서리 속성(33) 호출이 Win10에서
  무시되고 창이 정상 생성되는지 (현재 `_` 무시 — 스모크 필요).

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Acceptable | 디바운스 상태머신 정확·테스트 견고. 단 중복 실행·자동실행 경로 결함이 조건부로 핵심 기능에 영향 |
| Runtime Stability | Needs Work | 단일 인스턴스 부재 + 훅 내 파일 I/O가 침묵 중단 경로를 만듦 |
| Data Integrity | Acceptable | 정상 경로 저장·파싱 정확. 비원자 쓰기·복수 기록자만 약점 |
| Error Resilience | Acceptable | 업데이터 롤백·서명 검증·스테이징 정리가 탄탄. 훅 해제 감지 부재가 흠 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도. Win10에서 신형 DWM 속성이 무시값으로 처리되어 호환 유지 |
| Test Confidence | Acceptable | 핵심 로직 36개 통과. UI·훅·설정은 무테스트라 상한이 명확 |

**먼저 수정할 3개:** ISSUE-001 단일 인스턴스 뮤텍스 → ISSUE-002 Run 경로
인용 → ISSUE-003 사운드 경로 캐싱. (3개 모두 작고 국소적이며, 다음 릴리스
전에 들어가야 업데이트 배포 신뢰도가 완성된다.)

## 10. Addendum — 감사 이후 적용 내역 (2026-10-16)

본문은 감사 시점의 스냅샷이며, 아래는 그 이후 `main`에 적용된 후속 조치다.

- ISSUE-001~004 전부 수정: 단일 인스턴스 뮤텍스, Run 경로 인용, 훅 밖
  사운드 경로 검증(`ValidatedSound`), 원자적 저장 + `.bak` 폴백.
- `src/` 폴더 분할 완료: `app/`·`input/`·`config/`·`i18n/`·`ui/` + 기존
  `updater/`, `debounce.rs` (기능 누락 없음, 임시 진단 코드 제거됨).
- UI 미연결 설정 연결: 최소 채터 간격·로그 보관 수·합성 입력 무시·전체
  로깅 체크박스 (키별 임계값은 여전히 설정 파일 전용).
- 성능: 타이머 스냅샷-디프 갱신, 테마 캐시, 훅 무할당 사운드 재생.
- 의존성: `base64` 0.23, `sha2` 0.11, `windows` 0.62 (nullability 이식).
- 테스트 65개 통과 (`exe_smoke` 포함). 단 GUI 기동 직후 응답없음 리포트의
  근본 원인은 미확정으로 남는다.
