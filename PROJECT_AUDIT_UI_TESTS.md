# Project Audit — UI Layer, Test Infrastructure & Follow-up Verification

- 대상: `keyboard-chattering-fix` (`kcfix-rs`), 2026-10-17 작업 트리(`PROJECT_AUDIT.md` §10 수정 적용 후)
- 범위: 앞선 두 감사에서 정밀하게 보지 않은 영역
  - **UI 표현 계층:** `ui/{layout,widgets,paint,menu,keys,history,stats,tray,text,controls}`
  - **테스트 인프라**
  - **직전 수정분 자기 검토**
  - **파이프라인 잔여 항목:** `PROJECT_AUDIT_RELEASE.md`에 없던 것
- 이와 함께, 앞선 감사에서 "확인하지 못한 것"으로 남긴 항목을 **실제 바이너리로 검증하는 E2E**를 추가하고 실행했다.
- 코드 변경은 이 감사에서 확인된 결함 3건과 검증 장치 추가에 한정했다(§10).

---

## 1. Executive Summary

**전체 상태.** UI 계층은 "스냅샷 → 순수 렌더 → diff → 바뀐 컨트롤만 갱신" 구조가 일관되고, 렌더·레이아웃 테스트로 잘 방어되어 있다. 데이터 손상 경로는 없다.

이번 범위에서 의미 있는 결함은 두 곳에서 나왔다. 하나는 **직전 수정이 만든 회귀**이고, 다른 하나는 **헬퍼 CLI의 잠재 버그**다. 둘 다 이번에 수정하고 테스트로 고정했다. 나머지는 Low 등급의 UI 세부와 릴리스 게시 순서 문제다.

**전체 위험도: Low.**

**가장 중요한 문제**

1. **[UI-001]** 직전 수정 회귀: 훅 설치에 실패한 상태에서 Save를 누르면 `bFilterEnabled=0`이 저장된다. (Medium / Confirmed → 수정됨)
2. **[UI-002]** 헬퍼 CLI: 앱이 `--relaunch-arg --minimized` 형태로 보내면 헬퍼가 파싱하지 못하고 즉시 종료된다(잠재, 현재 경로 없음). (Low / Confirmed → 수정됨)
3. **[UI-003]** 릴리스가 에셋 업로드·검증 **전에** 공개된다. (Low / Confirmed, 미수정)
4. **[UI-004]** 트레이 컨텍스트 메뉴에 `TrackPopupMenu` 뒤 `WM_NULL` 처리가 없다. (Low / Likely, 미수정)

**데이터 손상/유실 가능성:** 없음. UI-001이 유일한 설정 오염 경로였고 수정됐다.

**가장 먼저 볼 영역:** 남은 항목은 모두 Low다. 릴리스 직전에 `scripts/e2e`의 두 스크립트를 돌리는 것이 가장 효과가 크다(§8).

---

## 2. Project Understanding (이번 범위)

```
WM_TIMER(500ms, UI 스레드)
  → stats::update_stats: SHARED 짧은 락 스냅샷(+HOOK_ACTIVE) → render_stats(순수) → diff_stats → 바뀐 컨트롤만 SetWindowTextW / set_toggle / tray::refresh_tip
  → history::refresh: 기록 탭이 보이고 log_seq가 바뀐 경우만 → render_events/render_keys(순수) → LVM_SETITEMCOUNT (LVS_OWNERDATA, LVN_GETDISPINFOW에서 셀 제공)
  → settings_form::tick / autosave_tick

WM_COMMAND → layout::spec(id)로 종류 판별 → widgets::flip_toggle / commands::run(MENU_*) / settings_form::on_save
WM_DRAWITEM → widgets::draw_item(토글·버튼·탭·상태 알약, ThemeColors + 캐시 브러시)
WM_PAINT → 더블 버퍼(카드·구분선) / WM_CTLCOLOR* → paint::cached_brush
WM_SETTINGCHANGE → 브러시 캐시 비움 + 테마 재조회 + 컨트롤 테마 재적용
```

- **선언적 컨트롤 표 `layout::SPECS`:** 생성, 번역(`text::apply_language`), 색(`paint::static_colors`), 페이지 전환(`is_visible`)이 모두 이 표를 순회한다.
- **좌표 변환:** 좌표는 96-DPI 논리값이고, `sc()`가 시스템 DPI로 변환한다.

---

## 3. Audit Coverage & Limitations

### 열람한 파일
- UI: `ui/widgets.rs`(상태·그리기 앞부분), `ui/keys.rs`, `ui/menu.rs`, `ui/layout.rs`(런타임 상태·카드·가시성), `ui/history.rs`(렌더·refresh·notify), `ui/stats.rs`, `ui/paint.rs`, `ui/tray.rs`, `ui/text.rs`, `ui/controls.rs`
- 파이프라인: `build.rs`, `.githooks/pre-push`, `.github/workflows/{release,windows-ci}.yml`, `scripts/{build_release.ps1,build_update_manifest.py,verify_update_manifest.py,setup_githooks.ps1}`
- 직전 수정분 전체: `input/hook.rs`, `settings_form.rs`, `window.rs`, `updater/{error,staging}.rs`, 헬퍼 `main.rs`/`lib.rs`
- git 이력: `v0.1.0`/`v0.2.0` 태그 시점의 헬퍼 CLI

### CodeGraph
- 이번 회차에서는 프롬프트 훅이 제공한 심볼 인덱스만 참고했고 `codegraph_explore`를 새로 호출하지는 않았다.
- 대상 UI 모듈이 앞선 회차의 CodeGraph 탐색으로 caller 관계가 확인된 상태였다(`update_stats` 7개 caller, `history::refresh` 3개 caller 등). 그래서 이번에는 파일을 직접 열람했다.

### 실행한 검증(실측)
| 검증 | 결과 |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings` | 통과 |
| `cargo test --workspace` | **128 passed**(이전 123) |
| `python -m pytest scripts/` | 7 passed |
| `scripts/e2e/hook_session_e2e.ps1`(현재 빌드) | 7/7 통과 |
| 같은 스크립트, **수정 전 HEAD 빌드**(임시 worktree, 음성 대조군) | 2건 실패: UI 정지 중 입력 **3,046ms 지연**, 세션 종료 시 카운터 **미저장** → 스크립트가 ISSUE-002/004 회귀를 잡아냄 |
| `scripts/e2e/update_swap_e2e.ps1`(현재 헬퍼) | 13/13 통과(연속 2회 교체) |
| 같은 스크립트, **GitHub에 게시된 v0.2.0 헬퍼**(`gh release download`) | 13/13 통과 → 새 앱이 이미 설치된 구 헬퍼로 연속 업데이트 가능함을 실측 |
| `--check-helper`로 게시된 v0.2.0 헬퍼 판정 | `helper ok … (0.2.0.0)`(ProductName·버전 범위 통과) |

### 확인하지 못한 것
- **실제 GitHub 게시를 거친 N → N+1 업데이트**(네트워크·서명 구간 포함). 교체 구간은 E2E로, 서명·매니페스트 구간은 단위 테스트로 각각 검증했다. 다만 새 태그를 게시하지 않는 한 둘을 이은 한 번의 흐름은 실행할 수 없다.
- **실제 물리 키보드 채터링.** 합성 F24 입력으로 대체했다(`bExcludeInjected=0`). 실제 하드웨어 입력은 수동 확인이 필요하다.
- **실제 로그오프.** `WM_QUERYENDSESSION`/`WM_ENDSESSION`을 보낸 뒤 강제 종료하는 방식으로 대체했다.
- **125%/150% DPI, 다크/라이트 전환의 시각 확인.** GUI 육안 확인을 하지 않았다.

---

## 4. High-Risk Issues

### [UI-001] 훅 설치 실패 시 보호 토글이 "꺼짐"으로 동기화되어, 다음 Save가 `bFilterEnabled=0`을 저장함 — 직전 수정의 회귀

- **위치:** `src/ui/stats.rs` `update_stats` / `render_stats`, `src/ui/settings_form.rs` `on_save`
- **우선순위:** Medium
- **신뢰도:** Confirmed(코드 흐름)
- **문제:** 직전 수정(G1)에서 상태 표시가 훅 부재를 반영하도록 스냅샷의 `enabled`를 `g.enabled && HOOK_ACTIVE`로 바꿨다. 그런데 `update_stats`는 같은 값을 `set_toggle(ID_TOGGLE_ENABLE, …)`로 **스위치에도** 반영하고, `on_save`는 그 스위치 값을 `enabled`로 읽어 디스크에 저장한다.
- **발생 조건:** 훅 설치가 실패한 세션에서 사용자가 설정을 바꾸고 Save를 누른다.
- **영향:** 다음 기동에서 훅이 정상 설치되더라도 보호가 꺼진 채로 시작한다. 사용자는 그 사실을 알아채기 어렵다.
- **근거:** `stats.rs`의 `set_toggle(hwnd, ID_TOGGLE_ENABLE, rendered.enabled)`, `settings_form.rs`의 `let enabled = get_toggle(ID_TOGGLE_ENABLE)` → `new_settings.enabled = enabled` → `config::save`.
- **반증 확인:** `persist_live`는 `g.enabled`를 직접 읽으므로 안전하다. 하지만 `on_save`는 토글 값을 쓰므로 보호되지 않는다.
- **수정(적용):** `StatsSnapshot.hook_active`를 분리했다. `RenderedStats.enabled`는 스위치 위치이고 `RenderedStats.active`는 `enabled && hook_active`다. 배지·트레이는 `active`를 쓰고, 스위치는 `enabled`를 쓴다.
- **회귀 테스트(추가):** `failed_hook_shows_off_but_keeps_the_switch_on` — 훅이 없으면 배지는 "Protection off"이고, 스위치(`enabled`)는 true이며, diff가 status 변경을 감지한다.

### [UI-002] `--relaunch-arg <하이픈으로 시작하는 값>`을 헬퍼가 파싱하지 못함

- **위치:** `src/updater/staging.rs` `helper_args`, `crates/kcfix-updater/src/main.rs` `Cli`
- **우선순위:** Low(현재 도달 경로 없음 — `--minimized`로 앱을 띄우는 곳이 없음)
- **신뢰도:** Confirmed(계약 테스트로 재현)
- **문제:** 앱은 `--relaunch-arg`와 `--minimized`를 별도 인자로 보냈다. clap 4는 `--minimized`를 알 수 없는 플래그로 해석해 오류로 종료한다.
- **영향(도달 시):** 앱은 이미 종료 중인데 헬퍼가 즉시 실패한다. 그러면 업데이트도 되지 않고 필터도 실행되지 않는다.
- **반증 확인:** 구 헬퍼(v0.1.0/v0.2.0)도 같은 clap 정의라 헬퍼 쪽 수정으로는 해결되지 않는다. 앱이 `--relaunch-arg=--minimized` 형태로 보내면 모든 clap 4 헬퍼가 수용한다.
- **수정(적용):** `helper_args`가 `=` 형태를 쓰도록 바꿨다.
- **회귀 테스트(추가):**
  - 헬퍼: `accepts_every_flag_the_app_sends`(앱이 보내는 전체 명령줄이 파싱됨), `hyphen_values_need_the_equals_form`(공백 형태는 거부됨을 고정)
  - 앱: `helper_args_pass_exit_timeout_and_hash`에서 `=` 형태를 검증

### [UI-003] 릴리스가 에셋 업로드·검증 전에 공개됨

- **위치:** `.github/workflows/release.yml` "Publish GitHub Release assets idempotently"
- **우선순위:** Low
- **신뢰도:** Confirmed
- **문제:** `gh release create`가 초안이 아닌 공개 릴리스를 만들고, 그 뒤에 에셋을 업로드하며, 검증은 마지막에 한다.
- **발생 조건 / 영향:**
  - 공개와 업로드 사이 구간에는 `releases/latest/download/update.json`이 404를 반환한다. 이때 "업데이트를 확인"한 사용자는 "게시된 업데이트가 아직 없습니다"라는 잘못된 안내를 받는다.
  - 검증이 실패해도 결함 있는 릴리스가 이미 "latest"로 공개되어 있다. 클라이언트는 서명·해시 검증으로 fail-closed되므로 보안 영향은 없고, 업데이트 실패 안내만 나온다.
- **반증 확인:** `concurrency: release`는 릴리스끼리 겹치는 것만 막고 공개 시점 문제는 막지 않는다. `PROJECT_AUDIT_RELEASE.md`에는 이 항목이 없다.
- **권장 수정:** `gh release create --draft` → 업로드 → `gh release download`(인증)로 에셋 검증 → `gh release edit --draft=false` → 공개 URL을 재검증하는 순서로 바꾼다.
- **필요한 검증:** 포크 저장소에서 `v0.0.1-test`가 아닌 `vX.Y.Z` 형식 태그로 1회 리허설을 한다. CI 변경이라 로컬에서는 실행할 수 없어 이번에는 적용하지 않았다.

### [UI-004] 트레이 컨텍스트 메뉴 뒤 `WM_NULL` 미전송

- **위치:** `src/ui/tray.rs` `show_tray_menu`
- **우선순위:** Low
- **신뢰도:** Likely(MS `TrackPopupMenu` 문서의 Remarks에 명시된 동작, 재현은 안 함)
- **문제:** 알림 영역 메뉴는 `SetForegroundWindow` 뒤 `TrackPopupMenu`를 호출하고, 그다음 자기 창에 `WM_NULL`을 보내 작업 전환을 강제해야 한다. 그러지 않으면 두 번째로 연 메뉴가 바로 닫히거나, 바깥을 클릭해도 닫히지 않는 경우가 있다.
- **권장 수정:** `TrackPopupMenu` 직후 `PostMessageW(hwnd, WM_NULL, 0, 0)`.
- **필요한 검증:** 수동으로 트레이 메뉴 열기 → Esc → 다시 열기를 3회 반복한다.

---

## 5. Potential Functional Gaps

| # | 분류 | 내용 |
|---|---|---|
| UG1 | Confirmed Gap | `keys::vk_name`에 **한/영(0x15), 한자(0x19)** 가 없어 `0x15`처럼 표시되고 기록 탭 검색도 되지 않는다. 한국어 사용자에게 흔한 키다. `0xE5`(ProcessKey) 등도 같다. 표시 문제일 뿐 필터 동작과는 무관하다. |
| UG2 | 추정 | 기록 탭 시각은 `기동 시 벽시계 + Instant 경과`로 계산한다. 시계 동기화나 수동 변경, 절전 복귀 뒤에는 표시 시각이 어긋날 수 있다. 차단 판정에는 영향이 없다. |
| UG3 | 추정 | `WM_SETTINGCHANGE`는 테마와 무관한 브로드캐스트(환경 변수 변경 등)에도 브러시 캐시 재생성과 전체 다시 그리기를 한다. 성능 영향은 미미하다. |
| UG4 | Confirmed Gap | 앱의 RFC3339 파서는 시간대 오프셋을 무시한다(`+09:00`을 UTC로 해석). 빌더는 항상 `Z`를 쓰므로 현재 도달하지 않는다. 다른 도구로 매니페스트를 만들 때만 문제가 된다. |
| UG5 | Confirmed Gap | `verify_update_manifest.py`의 URL 핀은 `https://github.com/` 접두사만 확인한다(앱은 저장소 경로까지 정확히 일치). `release.yml`의 인라인 assert가 태그·파일명은 보완하지만 저장소 경로는 보완하지 않는다. 서명 키가 별도라 보안 영향은 없다. |
| UG6 | Likely Gap | 훅 생존 감시(이전 G5)는 여전히 없다. 다만 E2E에서 UI 스레드를 정지시켜도 필터가 유지됨을 확인했으므로 주된 제거 원인은 사라졌다. |

---

## 6. Documentation Mismatches

- `CLAUDE.md`: "`second_acquisition_fails_while_first_held`는 실제 트레이 앱 옆에서 실패" → 고유 뮤텍스 이름으로 해소하고 문구를 고쳤다(§10).
- `CLAUDE.md`/`GEMINI.md`/`README.md`: E2E 절차와 `--check-helper` 안내가 없었다 → 추가했다.
- 그 밖에 이번 범위의 UI 모듈 주석과 실제 동작 사이의 불일치는 발견하지 못했다(`history.rs`의 가상 리스트 설명, `stats.rs`의 락 규칙 설명, `widgets.rs`의 토글 상태 설명 모두 일치).

---

## 7. Recommended Fix Plan

### Phase 1 — Immediate
- 없음(UI-001, UI-002는 적용 완료).

### Phase 2 — Stability
1. **UI-003:** 릴리스를 초안 → 검증 → 공개 순서로 바꾸고 포크에서 리허설한다.
2. **UI-004:** 트레이 메뉴 뒤 `WM_NULL`을 보낸다.
3. **UG1:** `vk_name`에 `0x15 → "Hangul"`(한국어 UI에서는 "한/영"), `0x19 → "Hanja"`, `0xE5 → "Process"`를 추가하고 `names_cover_*` 테스트를 확장한다.

### Phase 3 — Structural
4. **UG5:** `verify_update_manifest.py`에 저장소 경로 핀을 추가해 앱 규칙과 맞춘다.
5. **UG4:** 앱 파서가 `Z`가 아닌 오프셋은 거부(fail-closed)하도록 하고 테스트로 고정한다.
6. **E2E의 CI 편입 검토:** `windows-latest`는 대화형 데스크톱이 있어 LL 훅이 설치될 가능성이 높다. `workflow_dispatch` 전용 잡으로 먼저 시험한 뒤 릴리스 워크플로의 게이트로 올린다.

---

## 8. Test Recommendations

### 이번에 추가된 검증(모두 실행·통과)

**Unit**
- `stats::failed_hook_shows_off_but_keeps_the_switch_on` — 입력: `hook_active=false`, `enabled=true` → 기대: badge "Protection off", `enabled=true`, `active=false`, status diff 발생
- `instance::second_acquisition_fails_while_first_held` — 테스트별 뮤텍스를 사용한다. 두 번째 획득은 None이고, 가드를 해제하면 다시 획득된다. 실제 앱이 떠 있어도 통과한다.

**Contract**
- 헬퍼 `accepts_every_flag_the_app_sends` — 앱 명령줄 전체(`--pid --current --replacement --timeout-secs 120 --sha256 … --relaunch-arg=--minimized`)가 파싱되는지 확인한다.
- 헬퍼 `hyphen_values_need_the_equals_form` — 공백 형태는 거부됨을 고정한다.

**Smoke (`tests/exe_smoke.rs`)**
- `check_helper_reports_missing_helper` — 앱만 복사한 폴더에서 `--check-helper`가 exit 1이고 stderr에 `kcfix-updater.exe`가 나오는지 확인한다.
- `check_helper_accepts_the_workspace_helper` — 워크스페이스 헬퍼를 옆에 두면 exit 0이고 `helper ok … 0.2.0.0`이 출력되는지 확인한다.

**E2E (`scripts/e2e`)**
- `hook_session_e2e.ps1` — F24 6회 입력 중 2회 차단(`stat.87=6,2`), UI 스레드를 정지한 상태의 입력 묶음이 2.5초 안에 처리되는지, `WM_ENDSESSION` 뒤 강제 종료해도 저장되는지, 외부 `key.41=120` 편집이 보존되는지 확인한다.
- `update_swap_e2e.ps1` — 연속 2회 교체에서 마커 설치, 백업·스테이징 잔재 없음, 재실행 핸드셰이크, 교체 후 `--check-helper` 통과를 확인한다. `-HelperExe`로 게시된 구 헬퍼를 검증할 수 있다.

### 남은 권장 테스트
- **Platform-specific(수동, 릴리스 체크리스트):**
  - 실제 기계식 키보드에서 의도적으로 빠르게 연타했을 때 차단 수가 증가하는지
  - 실제 로그오프 → 재로그인 후 `iTotalPresses`가 유지되는지
  - 트레이 메뉴 3회 반복(UI-004)
  - 150% DPI 육안 확인
- **Integration(UI-003 수정 후):** 포크 릴리스 리허설에서 초안 상태 동안 `releases/latest/download/update.json`이 이전 릴리스를 가리키는지 확인한다.
- **Unit(UG1 수정 시):** `vk_name(0x15) == "Hangul"`, `key_matches(0x15, "hang")`.

---

## 9. Final Assessment

| 항목 | 평가 | 근거 |
|---|---|---|
| Functional Correctness | **Good** | UI 렌더·diff·가상 리스트 로직은 순수 함수와 테스트로 방어된다. 회귀(UI-001)는 수정했다. |
| Runtime Stability | **Good** | UI 스레드를 정지시켜도 필터와 시스템 입력이 유지됨을 실측했다(음성 대조군 3,046ms 지연 대비). |
| Data Integrity | **Good** | 세션 종료 저장과 외부 편집 보존을 E2E로 확인했다. 설정 오염 경로(UI-001)를 제거했다. |
| Error Resilience | **Acceptable** | 헬퍼 계약을 테스트로 고정했다. 릴리스 게시 순서(UI-003)는 남아 있다. |
| Cross-platform Robustness | **Acceptable** | Windows 전용 설계다. DPI·테마는 육안 확인을 하지 않았다. |
| Test Confidence | **Good** | 단위 128 + pytest 7에 더해, 회귀를 실제로 잡아냄이 확인된 E2E 2종이 있다. 다만 E2E는 수동 실행이다. |

### 실제로 먼저 처리할 3개
1. **UI-003** 릴리스를 초안 → 검증 → 공개 순서로 바꾸기(다음 태그 전에)
2. **릴리스 체크리스트에 `scripts/e2e` 두 스크립트 추가** — 특히 `-HelperExe`로 직전 게시 헬퍼를 검증
3. **UI-004 + UG1** 트레이 메뉴 `WM_NULL`, 한/영·한자 키 이름 추가(작고 사용자 체감이 큰 수정)

---

## 10. 적용 내역

| 항목 | 변경 |
|---|---|
| UI-001 | `StatsSnapshot.hook_active`, `RenderedStats.active` 분리. 배지·트레이는 `active`, 스위치는 `enabled` |
| UI-002 | `helper_args`가 `--relaunch-arg=VALUE` 형태 사용. 헬퍼 쪽 계약 테스트 2건 |
| 테스트 인프라 | `instance::try_acquire_named` 분리. 단일 인스턴스 테스트가 실제 앱 옆에서도 통과 |
| 검증 장치 | `--check-helper` CLI(`updater::describe_helper`). `tests/exe_smoke.rs` 2건 |
| E2E | `scripts/e2e/{common,hook_session_e2e,update_swap_e2e}.ps1` + `README.md` |
| 문서 | `CLAUDE.md`/`GEMINI.md`/`README.md`에 E2E·`--check-helper` 안내 |

---

## 11. 후속 적용 내역 (v0.2.1)

§4~§7의 미수정 항목을 모두 적용했다.

| 항목 | 상태 | 변경 |
|---|---|---|
| UI-003 릴리스가 업로드 전에 공개됨 | **해결** | `release.yml`: 매니페스트↔빌드 EXE 대조 → 초안 생성(에셋 첨부) → 이름·크기 확인 → 공개(`make_latest`) → 공개 URL 재검증(CDN 지연 대비 재시도). 같은 태그 재실행은 초안 교체/공개본 에셋 갱신 |
| UI-004 트레이 메뉴 `WM_NULL` | **해결** | `show_tray_menu`가 `TrackPopupMenu` 직후 `WM_NULL`을 전송 |
| UG1 한/영·한자 키 이름 | **해결** | `vk_name`: `0x15 Hangul`, `0x19 Hanja`, `0xE5 IME` + 검색 테스트 |
| UG4 만료 시각의 시간대 | **해결** | 앱 파서가 `Z`/`+00:00`(+소수 초)만 수용하고 다른 오프셋·표기 없음·비ASCII는 거부(fail-closed). 비ASCII 입력에서의 바이트 슬라이싱 패닉 가능성도 제거. 테스트 `expiry_timestamp_accepts_only_utc_forms` |
| UG5 검증 스크립트의 URL 핀 | **해결** | `verify_update_manifest.py`가 앱 상수(`RELEASE_DOWNLOAD_PREFIX`, `ARTIFACT_FILENAME`)를 읽어 `태그/파일명`까지 정확히 일치를 요구. pytest 케이스 추가 |
| E2E의 CI 편입 | **해결(수동 트리거)** | `.github/workflows/e2e.yml`(`workflow_dispatch`): 훅/세션, 신규 헬퍼 교체, 게시된 헬퍼 교체 |
| UG2/UG3 | 보류 | 표시 시각 어긋남·`WM_SETTINGCHANGE` 재그리기는 영향이 미미한 추정 사항 |
| UG6 훅 생존 감시 | 보류 | 주된 제거 원인(UI 정지)이 사라져 우선순위 낮음 |

검증: `cargo test --workspace` 통과(메인 105 포함), `pytest scripts/` 7 통과(핀 케이스 확장), `clippy -D warnings`/`fmt` 통과,
`scripts/e2e` 2종을 0.2.1 빌드로 재실행해 전부 통과.
