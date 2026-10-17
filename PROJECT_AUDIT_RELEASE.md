# Project Audit — Release Pipeline & Supporting Scripts

- 대상: `keyboard-chattering-fix`의 릴리스·패키징·CI 범위
  (`scripts/`, `packaging/`, `.githooks/`, `.github/workflows/`, `build.rs`,
  `crates/kcfix-updater/build.rs`, `tests/version_sync.rs`, 업데이트 클라이언트 연동부)
- 감사일: 2026-10-17, 코드 수정 없이 읽기·CodeGraph·안전 실행 검증만으로 감사
- 관점: 앞선 본 감사(`PROJECT_AUDIT.md`)와 동일한 기능·런타임 안정성 관점.
  본 감사가 다루지 않은 릴리스 E2E를 보완한다.
- 전체 위험도: **Acceptable**. 서명·해시 이중 검증 체인은 견고하고,
  감사에서 발견된 R-001(게시 EXE 미대조)·R-002(태그 글롭)는 모두 해소했다(§10).
  잔여 과제는 코드 사이닝 등록과 실제 게시 1회전 확인이다.

## 1. Executive Summary

릴리스 파이프라인의 보안 핵심(Ed25519 서명 → SHA-256 이중 검증 → 핀 고정 URL →
버전·크기·만료 검사)은 설계와 구현이 일치하고, 빌더(Python)↔검증자(Rust)↔사후
검증자(Python) 상호 운용을 실행으로 확인했다. 이전 본 감사에서 고친 롤백 재시도가
릴리스 게이트(`cargo test --workspace`)의 신뢰도에도 직결된다.

가장 중요한 문제 2개:

1. [R-001] 게시된 EXE 에셋의 해시 미검증 — 릴리스 검증 단계가 `update.json`만
   확인하고 정작 배포되는 바이너리의 해시를 대조하지 않음 (Medium / Confirmed)
2. [R-002] `v*` 태그 글롭 과발동 — 버전이 아닌 `v` 태그도 전체 릴리스를 돌리고
   크게 실패함 (Low / Likely)

데이터 손상/유실 가능성: 사용자 데이터 파괴 경로는 없다. 현실적 위험은 잘못된
바이너리가 서명된 매니페스트와 함께 게시되는 경우(R-001)이며, 클라이언트는 이를
다운로드 단계에서 거부하므로 실제 설치로는 이어지지 않는다(방어 확인됨).

제안된 수정은 §10과 같이 전부 적용됐다(릴리스 검증 EXE 대조·태그 제한·조건부 서명·
스크립트 테스트·헬퍼 버전 검사).

## 2. Project Understanding

### 목적

태그 푸시 한 번으로 테스트 → 빌드 → 서명 매니페스트 생성 → GitHub Release 게시 →
게시물 재검증을 수행하고, 클라이언트가 서명된 매니페스트로 안전하게 자동
업데이트하도록 한다. 코드 사이닝(Authenticode) 인프라는 있으나 릴리스에서는
미사용(`-NoSign` 고정)이며, 무결성은 Ed25519 매니페스트가 담당한다.

### 주요 entrypoint

- `.github/workflows/release.yml` — `v*` 태그 푸시 진입점
  (버전 대조 → `cargo test --workspace` → `build_release.ps1 -NoSign` →
  `build_update_manifest.py` → `gh release upload --clobber` → 게시물 재검증)
- `scripts/build_release.ps1` — 버전 메타데이터 검사 → 릴리스 빌드 → dist 복사 →
  패키징 스모크 → (선택) 서명 → zip
- `scripts/build_update_manifest.py` — EXE 해시·크기 측정 → 365일 만료 부여 →
  Ed25519 서명 → `update.json` 생성 (비밀키는 Actions secret에서만 읽음)
- `scripts/verify_update_manifest.py` — 게시물 사후 검증 (서명 + 필드 정합)
- `scripts/setup_githooks.ps1` — `.githooks` 활성화 (fmt + clippy + check)
- `build.rs` / `crates/kcfix-updater/build.rs` — 버전 리소스·아이콘·실행 수준 매니페스트

### 핵심 모듈·파일

| 파일 | 역할 |
|---|---|
| `release.yml` | 태그 파이프라인 전체 편성, 버전 게이트, 테스트 게이트, 멱등 게시 |
| `build_release.ps1` | 버전 3성분 검사, 빌드, 스모크(exit 코드), 서명/검증, zip |
| `build_update_manifest.py` | 정준 직렬화 서명, HTTPS·태그 형식·만료 양수 검증 |
| `verify_update_manifest.py` | 앱 내장 공개키 추출, 서명 검증, 필드 집합·태그 정합 |
| `version_sync.rs` | 버전 3위치 동기화, LICENSE, README 업데이트 안내, 아티팩트명 3처 일치 |
| `staging.rs`/`http.rs`/`manifest.rs` | 클라이언트 측 핀 고정·이중 해시·단일 비행 (본 감사에서 검증됨) |

### 데이터 저장 방식

- 릴리스 산출물: `dist/` (빌드 시 생성, `KeyboardChatteringFix.exe`·
  `kcfix-updater.exe`·`KeyboardChatteringFix.zip`, 워크플로에서 `update.json` 추가)
- 서명 비밀키: GitHub Actions secret(`KCFIX_UPDATE_PRIVATE_KEY_B64`)에만 존재,
  저장소에 커밋되지 않음 (스크립트는 환경변수에서만 읽음 — 코드로 확인)
- 스테이징·백업: `%TEMP%` 고유 경로 (본 감사 범위, 이중 해시 검증됨)

### 외부 의존성

- `actions/checkout@v7`·`setup-python@v7`(3.15)·`dtolnay/rust-toolchain@stable`·
  `gh` CLI·`cryptography>=50`·`signtool.exe`(미사용 경로)·`timestamp.digicert.com`(미사용 경로)
- 네트워크: `github.com` 릴리스 API·에셋 다운로드, 타임스탬프 서버(미사용)

### 핵심 실행 흐름

`git tag vX.Y.Z` → 태그≠`src/config.rs::VERSION`이면 즉시 실패 →
`cargo test --workspace` → `cargo build --release` 2개 바이너리 →
`dist/` 복사·스모크(exit 0)·zip →
`build_update_manifest.py`(dist EXE 해시·서명) → `gh release upload --clobber` 4종 →
게시된 `update.json` 다운로드·서명 재검증·태그·URL 접미사 assert

클라이언트 측: `MANIFEST_URL(latest/download/update.json)` 수신 →
서명·핀 고정 URL·버전 비교·크기·해시 형식·만료 검사 →
`%TEMP%` 다운로드 후 크기·해시 일치 → 헬퍼 스테이징 →
헬퍼가 교체 직전 해시 재검증 → 교체 → 60초 readiness →
실패 시 롤백·이전 버전 실행 (재시도 강건화됨)

## 3. Audit Coverage & Limitations

### 실제 확인한 모듈

위 표의 전 파일 전문 + `windows-ci.yml` + `docs/UPDATE.md`(서명·키교체·포크 절차) +
CodeGraph의 updater 호출관계(`launch_helper`·`resolve_helper`·`HELPER_FILENAME`·
`MANIFEST_URL`·`ARTIFACT_FILENAME`·`prepare_update`·`download_and_verify`).

### CodeGraph로 분석한 호출 관계

- `launch_helper` 호출자 2곳(`staging.rs` 내부 `apply_update`, `updater.rs` 리익스포트),
  `resolve_helper` 호출자 3곳, `HELPER_FILENAME` 참조 2곳 — 헬퍼 탐색 범위가
  앱 옆 폴더 우선 + dev 트리 폴백 4곳임을 확인.
- `check_for_update → parse_and_verify_manifest → is_newer` 호출 사슬 재확인.
- `download_and_verify`의 `take(size+1)`·크기 엄격 일치·해시 비교 순서 확인.

### 실행한 테스트·검증 (실측)

- `python scripts/build_update_manifest.py --help` — 인자 표면 정상.
- `py_compile` 3종 통과.
- Rust 테스트 픽스처(v99.0.0 서명 문서)를 추출해 `verify_update_manifest.py` 실행 —
  exit 0 (빌더↔Rust 검증자↔Python 검증자 3자 상호 운용 확인).
- 동일 문서 1바이트 변조본 — exit 1, `InvalidSignature`로 거부 (음성 검증).
- Python 3.15.0 로컬 존재 확인 (릴리스 핀 `3.15` 유효).
- `.githooks/pre-push` LF·BOM 없음 확인 (`.gitattributes`의 `eol=lf`와 일치).
- 실행하지 않은 것: `build_release.ps1`(전체 빌드, 느리고 `dist/` 산출물 발생),
  `setup_githooks.ps1`(git 설정 변경), 실제 태그 푸시·릴리스 워크플로,
  `generate_icons.py`(에셋 덮어쓰기 발생).

### 확인하지 못한 환경/외부 서비스

- GitHub Actions 러너에서의 실제 릴리스 1회전, `latest/download` 리다이렉트,
  `setup-python@v7`·`cryptography>=50` 핀의 유효성, 첫 릴리스 게시 전 404 경로.

### 분석상의 한계

- 릴리스 워크플로는 정적 분석 + 스크립트 단위 실행으로만 검증했다.
  빌드 스크립트 전체 실행과 실제 게시 E2E는 미수행으로 명시한다.

## 4. High-Risk Issues

### [R-001] 게시된 EXE 에셋의 해시 미대조 — 해결됨 (§10)

- **위치:** `.github/workflows/release.yml` "Verify published manifest asset" 단계
  (63~68행)
- **우선순위:** Medium
- **신뢰도:** Confirmed (빠진 검사가 코드로 확정됨, 오업로드 발생 빈도는 미측정)
- **문제:** 게시 후 검증이 `update.json`만 내려받아 서명·필드·태그·URL 접미사를
  확인할 뿐, 정작 함께 올린 `KeyboardChatteringFix.exe`를 내려받아 매니페스트의
  `sha256`·`size`와 대조하지 않는다. 빌드 해시→업로드 사이에 파일이 바뀌거나
  잘못된 바이너리가 `--clobber`로 덮이면 이 게이트를 그대로 통과한다.
- **발생 조건:** 업로드 과정의 파일 뒤바뀜·손상 (저빈도).
- **영향:** 클라이언트는 다운로드 단계에서 해시 불일치로 거부하므로 잘못된 설치로는
  이어지지 않는다. 영향은 "깨진 릴리스가 게시된 채로 방치 + 사용자 업데이트 전멸"
  이다. 즉 사용자 피해는 없고 배포 사고가 된다.
- **근거:** 검증 단계의 명령 3줄에 EXE 다운로드·해시 비교가 없음(전문 확인).
  `verify_update_manifest.py`도 서명·필드만 검사한다(41~43행).
- **반증 확인:** 클라이언트 측 이중 검증(`download_and_verify` 크기 엄격 일치 +
  해시 비교 + 헬퍼 교체 전 재검증)은 본 감사에서 코드·테스트로 확인했다.
  그래서 설치 피해로는 번지지 않지만, 게시물 자체의 검증 공백은 그대로 남는다.
- **호출/영향 범위:** 릴리스 파이프라인 최종 단계. 수정 영향은 워크플로 5줄 내외.
- **권장 수정 방향:** 검증 단계에 EXE 다운로드 + `sha256sum`·바이트수 대조 추가.
  실패 시 릴리스를 자동으로 삭제(draft 전환)하거나 후속 커밋 없이 재실행하도록
  문서화한다.
- **필요한 회귀 테스트:** 워크플로 변경이므로 코드 테스트 대신 릴리스 체크리스트에
  "게시물 EXE 해시 = update.json sha256" 항목을 추가한다. 또는 검증 스크립트에
  `--expect-asset` 옵션을 넣어 해시까지 확인하도록 확장한다.

### [R-002] `v*` 태그 글롭 과발동 — 해결됨 (§10)

- **위치:** `.github/workflows/release.yml` 3~6행 (`tags: ["v*"]`)
- **우선순위:** Low
- **신뢰도:** Likely (발동 조건은 자명, 실발생 기록은 없음)
- **문제:** `v`로 시작하는 모든 태그(`vtest` 같은 오타 태그 포함)가 전체 릴리스
  파이프라인(테스트·빌드 포함)을 돌린다. 버전 대조 단계에서 크게 실패하므로
  오게시는 없지만 CI 자원 낭비 + 실패한 릴리스 실행 기록이 남는다.
- **발생 조건:** 비버전 `v` 태그 푸시 (오타로 현실적).
- **영향:** 시간·자원 낭비. 산출물·게시물 영향 없음.
- **근거:** 글롭 패턴과 버전 대조(`v$version` 엄격 비교) 코드의 조합으로 확정.
- **반증 확인:** 오게시로 번지지 않음은 버전 대조 throw로 반증됐다. 남는 것은
  발동 자체의 비용뿐이다.
- **호출/영향 범위:** 워크플로 트리거 1줄.
- **권장 수정 방향:** `tags: ["v[0-9]*.[0-9]*.[0-9]*"]` 수준으로 제한.
- **필요한 회귀 테스트:** 워크플로 변경이라 기존 테스트 불가. 태그 푸시 규칙을
  `docs/UPDATE.md` 릴리스 절차에 1줄 추가한다.

## 5. Potential Functional Gaps

- **Confirmed Gap — 릴리스 바이너리 미서명.** `release.yml`이 `build_release.ps1`을
  `-NoSign` 고정으로 호출한다(40행). 서명 인프라(`Get-SigningConfig`, 타임스탬프,
  `verify /pa`)는 완성돼 있으나 시크릿 미등록 상태라 우회된다. Ed25519가 무결성을
  담당하므로 보안 공백은 아니나, SmartScreen 평판·실행 경고 측면의 gap이다.
  서명하려면 시크릿 등록 후 플래그만 떼면 된다.
- **Confirmed Gap — 스크립트 의존성 미문서·요구파일 부재.**
  `cryptography`(매니페스트 2종)·`Pillow`(아이콘)에 대한 `requirements` 파일이 없고,
  `README`·`CLAUDE.md`·`GEMINI.md` 어디에도 `pip install`·`setup_githooks.ps1`
  실행 안내가 없다(전수 grep 확인). `release.yml`만 `cryptography>=50`을 설치한다.
  로컬에서 스크립트를 처음 돌리는 기여자는 `ModuleNotFoundError`로 시작한다.
- **Confirmed Gap — 검증자간 엄격도 분기.**
  `verify_update_manifest.py`는 필드 집합 정확 일치·태그 정합만 보고 URL 핀·크기
  상한·해시 형식·만료를 검사하지 않는 반면, 앱은 그 반대(알 수 없는 필드 무시 +
  전부 검사)다. 빌더가 필드를 추가하면 검증 스크립트만 깨진다. 보안 영향은 없다.
- **Confirmed Gap — 포크 시 업데이트 정지(문서화된 동작).**
  `release.yml`의 `--artifact-url`은 `github.repository`에서 유도되지만 앱의
  `RELEASE_DOWNLOAD_PREFIX`는 원본 고정이라, 포크는 코드 2곳 수정이 필요하다.
  `docs/UPDATE.md`·`model.rs` 주석에 명시돼 있어 장애가 아니라 이행 조건이다.
- **추정 — `resolve_helper`의 MZ 매직만 검사 + dev 트리 폴백.**
  앱 옆 헬퍼가 손상되면 `target/` 아래 오래된 헬퍼를 스테이징할 수 있다.
  실패 방향이 안전(실행 실패 → 교체 없이 대화상자)이라 실질 위험은 낮다.
- **추정 — 릴리스 워크플로 동시성 무제어.**
  두 태그를 연속 푸시하면 실행이 겹치고 `gh release create`가 loudly 실패한다.
  `windows-ci`의 `concurrency`와 달리 `release.yml`에는 없다. 자동 복구는
  재실행 1회로 끝나며 오게시는 없다.

## 6. Documentation Mismatches

- **`setup_githooks.ps1`·스크립트 의존성의 안내 부재 (Low).**
  `README` 빌드 섹션은 `cargo test`·`cargo build`만 안내하고, pre-push 게이트를
  켜는 `setup_githooks.ps1` 언급이 없다. 기여자는 게이트 없이 푸시하고 CI에서
  처음 실패를 보게 된다. `CLAUDE.md`·`GEMINI.md`가 게이트를 설명하면서도
  활성화 절차를 적지 않은 점도 같다.
- **불일치 아님 (확인됨).**
  - `docs/UPDATE.md`의 버전 2곳·태그 형식·서명 절차 서술이 `release.yml`·
    `build_release.ps1`·`version_sync.rs`와 일치한다.
  - `MANIFEST_URL`·`RELEASE_DOWNLOAD_PREFIX` 변경 시 2곳 동시 수정 주의가
    코드 주석(`model.rs`)·문서·테스트(`manifest.rs` 테스트 기대 URL)에 일관되게 있다.
  - 스모크 검사가 exit 코드만 보는 한계가 `build_release.ps1` 주석에 명시돼 있고,
    출력 검사는 `exe_smoke` 테스트가 담당한다(책임 분리됨).
  - `build.rs`의 아이콘 "없어도 빌드 계속" 주석과 실제 `packaging/app_icon.ico`
    존재가 일치한다(둘 다 정상 상태).

## 7. Recommended Fix Plan

### Phase 1 — Immediate

1. R-001: 릴리스 검증 단계에 게시 EXE 다운로드 + `sha256`·`size` 대조 (5줄 내외).
2. R-002: 태그 글롭을 버전 형태로 제한.

### Phase 2 — Stability

3. `scripts/requirements.txt`(또는 주석) + `README`에 스크립트 실행 전제조건과
   `setup_githooks.ps1` 1회 실행 안내 추가.
4. `verify_update_manifest.py`에 URL 핀·만료 검사를 앱과 동등 수준으로 확장하거나,
   "서명·형식 스모크"임을 docstring에 명시.
5. `release.yml`에 `concurrency: group: release / cancel-in-progress: false` 추가.

### Phase 3 — Structural

6. 코드 사이닝 시크릿 등록 (메커니즘 구현됨 — 시크릿만 등록하면 서명됨. `docs/UPDATE.md` 참고).
7. `resolve_helper`에 버전 리소스 최소 검사(선택, 실패-안전 유지).

실제 코드는 수정하지 않는다 (본 감사의 제약).

## 8. Test Recommendations

- **Integration — 게시물 E2E (R-001):** 워크플로 검증 단계에서
  `gh release download vX.Y.Z -p KeyboardChatteringFix.exe` 후
  `sha256sum` = `update.json`의 `sha256`, 바이트수 = `size`임을 assert.
  입력: 게시 직후 산출물. 기대: 셋이 모두 일치, 불일치 시 워크플로 실패.
- **Unit — 빌더 입력 검증:** `build_update_manifest.py`에 `--version v99 / --tag v98`
  불일치 입력 시 `ValueError` 종료를 assert하는 pytest 3건(버전·태그, HTTPS,
  만료 양수). 현재 검증 로직은 있으나 테스트가 0개다.
- **Unit — 검증자 음성 케이스:** 위변조 서명 거부(본 감사에서 수동 확인,
  exit 1·`InvalidSignature`)를 pytest로 고정. 필드 추가 시 거부 vs 앱 수용의
  분기도 문서화 테스트로 둔다.
- **Regression — 파이프라인 상수 동기화:** 기존 `version_sync.rs`
  (`updater_artifact_name_matches_release_pipeline` 등)를 유지하고,
  R-001 수정 시 검증 단계의 해시 대조 명령 존재 여부를 검사하는 케이스 추가 검토.
- **Concurrency — 릴리스 중복 실행:** 수동 절차로 대체 (두 태그 연속 푸시 시
  후행 실행의 `gh release create` 실패가 loudly 보고되는지 1회 확인).
- **Platform-specific — 해당 없음.** 릴리스 러너는 `windows-latest` 고정이며
  산출물은 Windows 전용이 설계 의도다.

## 9. Final Assessment

| 영역 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | Good | 서명·해시·핀·버전·만료 체인이 코드와 일치, 3자 상호운용 실행 확인 |
| Runtime Stability | Good | 릴리스 단계는 실패 시 loudly 실패(침묵 성공 없음) |
| Data Integrity | Good | 빌드 해시→게시 일치 + 게시물 EXE 재검증(R-001 해소) |
| Error Resilience | Good | 버전 대조·테스트 게이트·서명 검증이 태그 오용을 차단 |
| Cross-platform Robustness | Acceptable | Windows 전용이 설계 의도, 러너 고정 |
| Test Confidence | Acceptable | Rust 측 79/79 + 픽스처 상호운용 + pytest 7건·핀 테스트. 실제 게시 E2E는 잔여 |

**먼저 수정할 3개:** R-001 게시 EXE 해시 대조 → R-002 태그 글롭 제한 →
Python 스크립트 요구사항·테스트 추가. (1·2번은 워크플로 수 줄로 끝나고,
다음 릴리스 전에 들어가야 배포 신뢰도가 완성된다.)

## 10. Addendum — 감사 제안 적용 내역 (2026-10-17)

본문 §1~§9는 감사 시점의 스냅샷이며, 아래는 그 제안을 코드에 적용한 내역이다.
`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo check --workspace --all-targets`, `cargo test --workspace`,
`python -m pytest scripts/` 모두 통과 (Rust 79개 + pytest 7개).

- R-001: 릴리스 검증 단계에 게시 EXE 다운로드 + `--expect-asset` 해시·크기 대조를
  추가했다. `verify_update_manifest.py`에 `validate_payload`(URL 핀·해시 형식·
  크기 범위·만료 fail-closed, 앱과 동등 수준)와 `check_asset`을 넣고,
  `version_sync.rs`에 `--expect-asset` 핀 테스트를 추가했다.
- R-002: 태그 글롭을 `v[0-9]*.[0-9]*.[0-9]*`로 제한하고, `docs/UPDATE.md` 릴리스
  절차에 태그 규칙 1줄을 추가했다.
- 조건부 서명: 시크릿(`SIGN_PFX_PATH`·`SIGN_CERT_SHA1`)이 있을 때만 서명하고,
  없으면 기존 `-NoSign` 동작을 유지한다. 릴리스에 `concurrency: group: release`
  (대기, 취소 없음)를 추가했다.
- 스크립트 기반: `scripts/requirements.txt` 신설, `README` 양 언어 빌드 섹션에
  의존성 설치·pytest·`setup_githooks.ps1` 안내 추가, `windows-ci.yml`에
  `setup-python` + `pip install -r` + `pytest scripts/` 단계 추가.
  `scripts/test_update_manifest.py` 7건(서명 Fixture 수용·위변조 거부·검증기
  음성 케이스·빌더 입력 검증·정준 직렬화)이 CI에서 돈다.
- 헬퍼 버전 스탬프: `kcfix-updater` 빌드에 `FileVersion`·`ProductVersion`을
  embed하고, `resolve_helper`가 앱 옆 복사본은 불일치 시 loudly 실패,
  dev 트리 폴백은 동일 기준으로 스킵하도록 했다. 스탬프 없는 구버전은
  업그레이드 호환을 위해 수용한다. `file_product_version`·호환성 단위 테스트
  3건을 추가했다(정규화 버그 1건을 테스트가 잡아 수정함).
- 로컬 ARM64 환경에서 `pip install -r scripts/requirements.txt`가
  `cryptography>=50` 휠 부재로 실패한다(x64 CI 러너에서는 정상).
  pytest 실행 자체는 기존 인터프리터로 통과하므로 추가 환경 작업을 하지 않았다.

재평가: Data Integrity → Good(R-001 해소), Test Confidence → Acceptable
(스크립트 테스트·핀 테스트 추가, 실제 게시 E2E는 잔여). 전체 평가는
Acceptable을 유지한다(코드 사이닝 미등록, 실제 게시 1회전 미수행이 잔여).
