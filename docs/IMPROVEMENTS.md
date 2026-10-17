# 개선·추가 기능 제안 (v0.2.2 기준)

- 작성일: 2026-10-17 · 대상 커밋: `b47d979` (v0.2.2)
- 방법: README·CLAUDE.md·`PROJECT_AUDIT.md` 확인 → CodeGraph(`codegraph_explore`)로 `Debouncer::decide`·`hook_proc`·`spawn_hook_thread`·`Settings`·`persist_live`·트레이 메뉴 열람 → 웹에서 유사 소프트웨어·펌웨어 디바운스 자료 조사
- 이 문서는 **제안 목록**이다. 항목마다 근거 위치(파일:줄)를 달았다(작성 시점 기준).

> **진행 상황**: 모든 항목을 **v0.3.0**에서 처리했다(변경 목록은 `CHANGELOG.md`). 항목별 결과:
>
> | # | 결과 |
> |---|---|
> | A1 | 1단계(차단된 Down 뒤 `held` 유지) + 2단계 떼기 확인 시간(`iReleaseDeferMs`: `Decision::Defer` → 훅 스레드 타이머 → injector 스레드 `SendInput`, 표식 `INJECT_MARK`). 키·마우스 공통 |
> | A2 | `VK_PACKET`/`VK_PROCESSKEY` 항상 통과·미집계 |
> | A3 | **제안 기각, 대신 `timeBeginPeriod(1)` 제거.** 검토 결과 `KBDLLHOOKSTRUCT.time`은 `GetTickCount` 기반이라 해상도가 10~16 ms로 QPC보다 나쁘다. 측정은 `Instant`(QPC)를 유지하고, 측정과 무관한 타이머 해상도 상향만 없앴다 |
> | A4 | 타이밍을 물리 키 슬롯(vk + 확장 플래그, 512칸)으로 분리. 통계·임계값은 vk 단위 유지 |
> | B1 | `WH_MOUSE_LL`, 켜 둔 동안만 설치(`bFilterMouse`) |
> | B2·B3 | [키·마우스] 탭(즉시 적용·저장, 간격 히스토그램 권장값, 기록 탭 더블클릭 연동) |
> | B4 | `sHotkeyToggle`/`sHotkeyCleaning`(고급 탭), 메뉴·트레이 메뉴에 표시 |
> | B5 | `sAutoDisableApps`/`bAutoDisableFullscreen`(설정 탭), 전경 창이 바뀔 때만 프로세스 조회, 상태 알약 "잠시 멈춤" + 홈 화면 사유 |
> | B6 | 청소 모드 자동 종료(`iCleaningTimeoutMin`)·종료 알림·Up 통과, 기록 CSV, 설정 내보내기/가져오기, 포터블 모드, 트레이 회색 아이콘(리소스 2) |
> | C1 | 워치독: 사용자 활동 중 훅이 10초간 무소식이면 표식 프로브 → 1초 내 미도착 시 훅 스레드 교체. **raw input 방식은 앱 창이 전경일 때 LL 훅이 입력을 못 받게 만들어 폐기**(실측) |
> | C2 | 관리자 창 감지·홈 안내, 관리자 권한으로 다시 실행(`--after-pid`), 관리자 권한 로그인 작업(XML, 배터리 조건·시간 제한 해제). 자동 실행 중복은 `--autostart`로 조용히 종료 |
> | C3 | 무작위 스트림 불변식 테스트(떼기 확인 포함), `features_e2e.ps1` |
> | D1 | README 기능·설정·설치·명령줄·FAQ 갱신 |
> | D2 | `CHANGELOG.md`, Scoop 버킷(`bucket/`), winget 매니페스트(`packaging/winget/`, 미제출), 패키지 관리자 설치본의 업데이트 안내, 코드 서명 안내(`docs/UPDATE.md`) |
>
> 실제 바이너리로 확인하지 못한 것: UAC 동의가 필요한 관리자 권한 재실행과 작업 등록(단위 테스트로 인자·XML만 검증), 실제로 훅이 제거되는 상황의 워치독 복구(판단 로직은 단위 테스트, 프로브 경로는 E2E 실행 중 정상 동작).

---

## 0. 요약 (우선순위)

| # | 제안 | 분류 | 효과 | 난이도 |
|---|---|---|---|---|
| A1 | 누르고 있는 중 생긴 바운스(중간 Up)로 키가 "떼어졌다가" 오토리핏까지 차단되는 문제 | 필터 정확도 | 높음 | 중 |
| A2 | `VK_PACKET`(0xE7) 등 "같은 VK가 서로 다른 글자"인 이벤트는 필터 대상에서 제외 | 필터 정확도 | 중 | 하 |
| A3 | 간격 측정에 `KBDLLHOOKSTRUCT.time` 사용 검토 (훅 호출 지연 제거) | 필터 정확도 | 중 | 하 |
| B1 | 마우스 버튼 채터링(더블클릭 고장) 필터 `WH_MOUSE_LL` | 기능 | **매우 높음** | 중 |
| B2 | 키별 임계값 UI (현재 파일 전용) + 기록 탭에서 "이 키에 임계값 설정" | 기능/UX | 높음 | 중 |
| B3 | 임계값 자동 추천(간격 분포 기반) — 경쟁 도구에 없는 차별점 | 기능 | 높음 | 중 |
| B4 | 전역 단축키(켜기/끄기, 청소 모드) | 기능 | 중 | 하 |
| B5 | 프로그램별·전체 화면 자동 비활성화 (게임·안티치트 대응) | 기능 | 중 | 중 |
| C1 | 훅 생존 감시(워치독)와 "보호 중" 표시의 실제 근거화 | 안정성 | 높음 | 중 |
| C2 | 관리자 권한 창(UAC 상승 앱)에서 필터가 안 걸리는 한계 문서화 + 선택적 상승 실행 | 안정성/문서 | 중 | 중 |
| D1 | README 과장·불일치 문구 정리 + 문제 해결(FAQ)·임계값 튜닝 가이드 | 문서 | 중 | 하 |
| D2 | winget/Scoop 배포, CHANGELOG | 배포 | 중 | 하 |

---

## 1. 웹 조사 결과: 유사 소프트웨어와 비교

### 1.1 조사한 도구

| 도구 | 플랫폼 | 방식 | 특징 |
|---|---|---|---|
| **KeyboardChatterBlocker** (FreneticLLC/mcmonkey, C#, MIT, ★≈1.2k) | Windows | LL 훅 | 키별 임계값(UI 탭), **마우스 버튼(좌/우/중/앞/뒤) + 휠 역스크롤 감지**, 채터 로그, `chatter.wav`, **단축키 6종**(토글/켜기/끄기/임시 켜기·끄기/창 표시), `hotkey_tempblock`(누르는 동안 전 입력 차단), **`auto_disable_programs`·`auto_disable_on_fullscreen`**, `measure_from`(기본 Press), `exclude_injected`, `minimum_chatter_time`, 통계 저장, MSI·Chocolatey 배포 |
| **KeyboardChatteringFix-Linux** (finkrer, Python) | Linux | evdev grab → uinput 재출력 | 단일 임계값(기본 30 ms, **release→press** 기준), systemd 유닛, 키보드 자동 탐지 |
| **keyboard-unchatter** (ZoserLock) | Windows(.NET) | 훅 | 통계. 알려진 문제: **Key Up이 삼켜져 게임에서 오동작**, "사용자 Up과 채터 Up을 구별 못 함" |
| **Unshaky** (macOS) | macOS | 이벤트 탭 | 기본 40 ms, 키별 설정, 메뉴바 차단 수, 라이브 디버그 창. 유지보수 모드 |
| **Windows BounceKeys** (FilterKeys `iBounceMSec`) | Windows 내장 | OS | 키를 뗀 뒤 일정 시간 같은 키 무시. **단일 값만**, SlowKeys/RepeatKeys와 상호 배타 |
| **QMK 펌웨어 디바운스** | 키보드 펌웨어 | 매트릭스 스캔 | `sym_defer_g`(기본), `sym_eager_pk`, `asym_eager_defer_pk` 등. eager=즉시 보고 후 무시, defer=안정될 때까지 대기(노이즈 내성) |

### 1.2 비교에서 나온 시사점

- **이 프로젝트가 이미 앞선 부분**: 훅 전용 스레드, 무할당 핫패스, Ed25519 서명 자동 업데이트, 원자적 설정 저장, 키별 차단 통계·최다 불량 키 진단, 다국어 자동 인식, release 기준 기본값(Linux 도구·BounceKeys와 같은 기준이며 KeyboardChatterBlocker의 기본 Press보다 느린 릴리스에 강함).
- **빠져 있는 부분(경쟁 도구 대비)**: 마우스 버튼, 단축키, 프로그램별/전체 화면 자동 비활성화, 키별 임계값 UI, 패키지 매니저 배포.
- **어느 도구에도 없는 기회**: 임계값 **자동 추천**. KeyboardChatterBlocker README도 "전역 0, 대상 키 300 ms로 두고 타이핑한 뒤 채터 로그의 최댓값을 올림(73 → 100)" 하는 **수동 절차**를 안내할 뿐이다. 이 프로젝트는 이미 차단 간격을 로그로 갖고 있어 자동화하기 쉽다(B3).
- **사용자 커뮤니티의 반복 이슈**: 게임 안티치트(VAC) 오탐 우려(KeyboardChatterBlocker #15), 베트남어 IME(EVKey/Unikey)와 충돌 → `Packet`·`Back` 키를 0으로 두는 우회(#55), 높은 임계값으로 좌클릭 더블클릭 불가, 로그인 화면 등 보호된 입력 영역 미적용.

---

## 2. 필터 정확도

### A1. 누르고 있는 중의 바운스 → 키가 끊기고 오토리핏까지 "채터"로 차단됨

`Debouncer::decide`([src/debounce.rs:171](../src/debounce.rs#L171))는 Down에만 필터를 걸고 Up은 항상 통과시킨다([src/debounce.rs:183-187](../src/debounce.rs#L183-L187)). 차단된 Down은 `held`를 세우지 않는다.

임계값 50 ms, `release` 기준에서 키를 **계속 누르고 있는데** 접점이 한 번 튀는 경우:

| 시각 | OS 이벤트 | 결과 | 앱이 보는 것 |
|---|---|---|---|
| 0 | Down | 통과, `held=true` | 누름 |
| 200 | Up (바운스) | 통과, `held=false`, `last_release=200` | **뗌** |
| 203 | Down (바운스) | gap 3 < 50 → 차단 | — |
| 233 | Down (오토리핏) | `held=false`라 리핏으로 인식 못 함, gap 33 → **차단, 채터로 집계** | — |
| 266 | Down (오토리핏) | gap 66 → 통과 | 다시 누름 |

결과: 게임에서 이동 키가 순간 끊기고(keyboard-unchatter가 "Known Issue"로 적은 바로 그 증상), 정상 오토리핏이 채터 통계를 부풀린다.

제안(작은 것부터):
1. **차단 직후 짧은 창 안에 오는 다음 Down은 리핏 후보로 취급**: 차단된 Down이 있으면 그 키를 "물리적으로 눌림" 상태로 보고(`held=true` 또는 별도 `suppressed_down` 플래그), 다음 Down은 리핏으로 통과시키지 않고 **조용히 무시**(통계 미집계). 이것만으로 233 ms 행의 오집계는 사라진다.
2. **선택 옵션 "떼기 지연(Release defer)"**: QMK `asym_eager_defer_pk`처럼 Up을 N ms 보류했다가 그 안에 Down이 오면 Up·Down을 둘 다 버린다. 훅은 Up을 차단(`LRESULT(1)`)하고, N ms 뒤에도 Down이 없으면 `SendInput`으로 Up을 재주입해야 하므로 (a) 재주입 이벤트에 자체 표식(`dwExtraInfo`) 부여 후 훅에서 통과, (b) 훅 스레드 타이머(`SetTimer` 스레드 타이머 — 훅 스레드에 메시지 루프가 이미 있음) 필요. 기본값은 꺼짐, 고급 탭 옵션으로.
3. 단위 테스트: 위 표의 시퀀스를 그대로 `decide` 테스트로 고정(`held_key_bounce_does_not_count_repeats_as_chatter`).

### A2. `VK_PACKET`(0xE7) 등은 필터 제외

IME·화면 키보드·일부 원격 도구는 유니코드 문자를 `VK_PACKET`으로 보낸다. 서로 다른 글자도 VK가 모두 0xE7이라, `bExcludeInjected=0`인 사용자는 빠르게 입력된 다른 글자가 "같은 키 채터"로 차단된다. KeyboardChatterBlocker의 베트남어 IME 충돌(#55)과 같은 계열이다.

- `decide` 진입부 또는 `hook_proc`([src/input/hook.rs:30](../src/input/hook.rs#L30))에서 `vk == 0xE7`이면 항상 통과(통계 제외). `VK_PROCESSKEY`(0xE5)도 같은 이유로 검토.
- `bExcludeInjected=1`이 기본이라 현재 대부분은 안전하지만, 이 옵션을 끄는 이유가 "원격/가상 키보드에서도 필터하고 싶다"인 사용자가 정확히 이 경우에 걸린다.

### A3. 간격 측정 시각: `start.elapsed()` → `KBDLLHOOKSTRUCT.time` 검토

현재는 콜백이 **실행된 시점**(`start.elapsed()`, [src/input/hook.rs:48,61](../src/input/hook.rs#L61))으로 간격을 잰다. 시스템이 바쁘면 이벤트가 큐에 쌓였다가 몰려서 전달될 수 있고, 그러면 실제로는 80 ms 간격인 두 입력이 수 ms 간격으로 보여 **정상 입력이 차단**될 수 있다. `kb.time`은 OS가 이벤트에 찍은 타임스탬프(ms)라 이 지연과 무관하다.

- 49.7일 주기 랩어라운드는 `u32::wrapping_sub`로 처리.
- ms 해상도면 채터 판정(수~수십 ms)에 충분하다.
- 바꾸면 `timeBeginPeriod(1)`([src/app/bootstrap.rs:105](../src/app/bootstrap.rs#L105))의 근거도 다시 볼 것: `Instant`(QPC)·`kb.time` 어느 쪽도 시스템 타이머 해상도에 좌우되지 않는다. Windows 10 2004 이후 이 설정은 프로세스별로 적용되고 상주 프로그램이 늘 켜 두면 전력 소모만 늘 수 있다. 다른 의존(예: 사운드 타이밍)이 없다면 제거하고, README의 "15.6 ms 지터 제거·마이크로초 정확도" 문구도 함께 고친다(D1).

### A4. (작은 것) 같은 VK를 공유하는 물리 키

메인 Enter와 숫자패드 Enter는 VK가 같고(`LLKHF_EXTENDED`로만 구별) 하나의 키로 집계·필터된다. 두 키를 번갈아 빠르게 누르면 채터로 차단될 수 있다. 영향은 작지만, 키 식별을 `vk | (extended << 8)` 같은 합성 키로 하면 통계도 정확해진다(배열 크기 512, 핫패스 비용 동일).

---

## 3. 기능 추가

### B1. 마우스 버튼 채터링 필터 (가장 수요가 큰 기능)

마우스 좌클릭 "더블클릭 고장"은 키보드 채터링보다 흔한 하드웨어 증상이고, KeyboardChatterBlocker가 지원하는 대표 기능이다.

- 같은 훅 스레드에 `WH_MOUSE_LL`을 추가 설치 → `WM_LBUTTONDOWN/UP`, `RBUTTON`, `MBUTTON`, `XBUTTON1/2`를 가상 VK(`VK_LBUTTON` 0x01 등)로 매핑해 **기존 `Debouncer`를 그대로 재사용**(VK 0x01~0x06은 키보드 훅에서 나오지 않으므로 충돌 없음).
- `MSLLHOOKSTRUCT.flags & LLMHF_INJECTED`로 합성 입력 제외.
- 마우스는 기본 꺼짐 + 버튼별 켜기(좌클릭에 높은 임계값을 주면 더블클릭이 불가능해진다는 경고 문구 필수). 권장 시작값 30~60 ms.
- 휠 역스크롤 감지는 후순위(방향 반전 + 짧은 간격).
- 마우스 이동 이벤트(`WM_MOUSEMOVE`)는 훅 진입 즉시 `CallNextHookEx` — 초당 수백~수천 번 오므로 **락을 잡기 전에** 걸러야 핫패스 규칙을 지킨다.

### B2. 키별 임계값 UI

현재 `key.XX`는 파일 전용이다([src/config/model.rs:42-49](../src/config/model.rs#L42-L49) `adopt_file_only_fields`). README는 이를 핵심 기능으로 소개하지만 실제 사용자는 16진 VK 코드를 알아야 한다.

- 고급 탭에 "키별 임계값" 목록(가상 ListView 재사용 가능) + "키 추가(다음에 누르는 키 캡처)" 버튼.
- **기록 탭 연동**: 키별 통계 행에서 "이 키에 임계값 설정…" — 문제 키를 찾은 그 자리에서 바로 조치.
- UI로 편집 가능해지면 `per_key_ms`는 더 이상 파일 전용 필드가 아니다. `adopt_file_only_fields`의 병합 규칙(파일 우선)을 "UI가 바꾼 키만 앱 값 우선"으로 바꿔야 실행 중 파일 편집 보존(ISSUE-005)과 충돌하지 않는다. 기존 `external_per_key_edit_survives_*` 테스트를 확장.

### B3. 임계값 자동 추천 (차별화 기능)

이미 차단 로그에 `gap_ms`가 있다([src/debounce.rs:21-26](../src/debounce.rs#L21-L26)). 여기에 **통과한 입력 중 임계값 근처 간격**도 집계하면 추천이 가능하다.

- 훅 쪽: 키별 고정 크기 히스토그램(예: 0~200 ms를 5 ms 버킷 40칸 × 256키, `u32` 약 40 KB, 할당은 기동 시 1회) — 핫패스에서는 인덱스 증가만.
- 분석(UI 스레드): 채터 간격은 보통 0~30 ms에 몰리고, 사람의 의도적 연타는 대개 그보다 길다. 두 분포 사이 골짜기를 찾아 "이 키는 약 N ms 권장"을 제시하고, 골짜기가 없으면(채터와 정상 연타가 겹침) "스위치 교체·청소 권장"을 띄운다.
- **놓친 채터 의심(near-miss) 지표**: 임계값을 조금 넘어 통과한 Down 수(예: 임계값~임계값+15 ms). 이 값이 크면 임계값을 올려야 한다는 신호다.
- 순수 함수(`recommend_threshold(&histogram) -> Recommendation`)로 만들어 HWND 없이 단위 테스트(CLAUDE.md 규칙과 일치).

### B4. 전역 단축키

- `RegisterHotKey`로 보호 켜기/끄기, 청소 모드 진입. 기본값은 미지정(충돌 방지), 설정 파일 또는 고급 탭에서 지정.
- 주의: 훅이 단축키 조합 자체를 채터로 막지 않도록 등록된 조합은 통과 경로를 확인.

### B5. 프로그램별·전체 화면 자동 비활성화

게임 안티치트 오탐 우려와 게임 내 입력 끊김(A1) 때문에 경쟁 도구 사용자들이 많이 찾는 기능이다.

- 500 ms 타이머에서 `GetForegroundWindow` → `GetWindowThreadProcessId` → `QueryFullProcessImageNameW`로 실행 파일 이름 확인 → 목록(`sAutoDisable=game.exe/other.exe`)에 있으면 `Shared.enabled`와 별개의 `suspended` 플래그를 세움(사용자 토글 상태를 덮어쓰지 않음).
- 전체 화면 판정: 전경 창 사각형 == 모니터 사각형(`MonitorFromWindow`) && 데스크톱/셸 창 아님.
- 상태 표시: "일시 중지됨 (game.exe)" 알약 + 트레이 툴팁.
- 이 확인은 **UI 스레드 타이머**에서 하고 훅은 플래그 하나만 읽는다(핫패스 유지). 전경 전환 순간을 빨리 잡고 싶으면 `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`.

### B6. 그 밖의 작은 기능

- **청소 모드 안전장치**: 일정 시간(예: 5분) 후 자동 해제 옵션 + 진입 시 트레이 풍선 알림. 현재 해제는 Enter 또는 버튼뿐([src/debounce.rs:174-180](../src/debounce.rs#L174-L180)). 청소 모드는 Up도 삼키므로, 진입 시점에 눌려 있던 키(예: 수정 키)는 앱 입장에서 "눌린 채"로 남을 수 있다 → 진입 시 Up은 통과시키는 것도 검토.
- **기록 내보내기**: 기록 탭 → CSV(시각, 키, 간격, 차단 여부). 수리·RMA 문의 시 증빙으로 유용.
- **설정 내보내기/가져오기**와 **포터블 모드**(실행 파일 옆 `settings.ini`가 있으면 그것을 사용).
- **트레이 아이콘 상태 표시**: 보호 꺼짐/일시 중지/청소 모드일 때 아이콘 자체를 바꿈(현재는 툴팁·메뉴 문구).

---

## 4. 안정성

### C1. 훅 생존 감시

훅을 전용 스레드로 옮겨 UI 정지에 의한 제거 위험은 줄었다([src/input/hook.rs:100-135](../src/input/hook.rs#L100-L135)). 그러나 훅 스레드도 `SHARED` 락에서 오래 기다리거나 시스템이 극도로 바쁘면 `LowLevelHooksTimeout`을 넘겨 OS가 **알림 없이** 훅을 뗄 수 있고, 그 경우 UI는 여전히 "보호 중"을 표시한다.

- 훅 콜백에서 `AtomicU64` 이벤트 카운터 증가(락 밖, 비용 무시 가능).
- UI 타이머(예: 5초마다)에서 `GetLastInputInfo().dwTime`이 계속 바뀌는데(=키보드·마우스 입력이 있었는데) 카운터가 멈춰 있으면 **훅 의심** → 훅 스레드 재시작(`HookThread::stop` + `spawn_hook_thread`) 및 `tracing::warn!`. 마우스 입력만 있을 때 오탐하지 않도록 B1의 마우스 훅 카운터와 합산하거나, 몇 회 연속일 때만 판단.
- 상태 알약을 "보호 중"(최근 N초 내 훅 이벤트 확인) / "확인 중"으로 나누면 표시가 실제 상태에 근거하게 된다.
- `hook_proc` 안의 `SHARED` 락 대기 시간 상한을 위해, UI 쪽 락 구간 길이를 디버그 빌드에서 측정·경고하는 것도 고려.

### C2. 관리자 권한 창과 보호된 입력 영역

비상승(medium IL) 프로세스의 LL 훅은 **관리자 권한으로 실행된 창이 전경일 때 그 입력을 보지 못한다**(UIPI). 작업 관리자, 관리자 권한 터미널, 일부 게임 런처·설치 프로그램에서 필터가 안 걸리고, 로그인 화면·UAC 보안 데스크톱은 어떤 사용자 공간 도구도 걸 수 없다(KeyboardChatterBlocker README도 같은 한계를 명시).

- README에 "알려진 한계" 절로 명시(D1).
- 선택 기능: "관리자 권한 창에서도 필터" 옵션 → 자동 시작을 Run 키 대신 **작업 스케줄러(가장 높은 수준의 권한으로 실행)**로 등록. `uiAccess=true`는 서명 + `Program Files` 설치가 필요해 현재 포터블 배포와 맞지 않으므로 후순위.
- 감지 보조: 전경 창이 상승 프로세스이고 우리는 아니면 상태 알약에 "이 창에서는 필터가 작동하지 않음" 표시(`OpenProcessToken` + `TokenElevation`, 타이머에서).

### C3. 테스트 보강 (개수 채우기가 아닌 불변식 위주)

- `decide`에 대한 **속성 기반 테스트**(`proptest`, dev-dependency): 임의 이벤트 열에 대해 (1) 청소 모드가 아니면 Up은 항상 통과, (2) `blocked_presses ≤ total_presses`, (3) 키별 합계 == 전체 합계, (4) 첫 Down은 항상 통과, (5) 로그 길이 ≤ `log_limit`.
- A1·A2·A3를 고칠 때 각 시나리오를 회귀 테스트로 고정.

---

## 5. 문서·배포

### D1. README 정리

- "1 ms 멀티미디어 타이머로 마이크로초 정확도" — 측정은 `Instant`(QPC) 기반이라 타이머 해상도와 무관하다. 문구를 사실대로 바꾸거나 A3와 함께 정리.
- "Per-Key Custom Overrides"가 "Helpful Tools & Customization"에 UI 기능처럼 적혀 있으나 파일 전용이다 → "설정 파일에서" 명시(B2 전까지).
- "Memory Footprint < 5MB" 배지 — 측정 방법·수치를 근거로 남기거나 배지 제거.
- 아키텍처 트리의 `i18n/ — Bilingual string tables`는 확장 가능한 언어 레지스트리 구조로 바뀐 v0.2.2 설명과 맞지 않는다.
- 새 절:
  - **임계값 튜닝 가이드**: 기본 50 ms(release 기준). 채터 간격은 대개 수~수십 ms, 웹 가이드들의 권장 시작값 20~50 ms, 문제 키만 키별로 올리기. 기록 탭에서 차단 간격 최댓값을 보고 약간 위로 잡는 절차.
  - **알려진 한계/FAQ**: 관리자 권한 창(C2), 로그인 화면, 게임 안티치트(입력 차단 도구 일반의 위험, 자동 비활성화 B5 안내), IME·원격 데스크톱(`bExcludeInjected`), 소프트웨어 필터는 증상 완화일 뿐 스위치 청소·교체가 근본 해결이라는 점, QMK/VIA 키보드라면 펌웨어 `DEBOUNCE` 조정이 더 낫다는 점.
- `git status`에 `PROJECT_AUDIT_RELEASE.md`·`PROJECT_AUDIT_UI_TESTS.md` 삭제가 커밋되지 않은 상태로 남아 있다 — 의도한 정리라면 커밋할 것.

### D2. 배포

- **winget** 매니페스트(`winget-pkgs` PR) 및 **Scoop** 버킷 — 경쟁 도구는 Chocolatey·MSI를 제공. 포터블 zip + 고정 URL 릴리스 구조라 Scoop이 가장 쉽다. 자동 업데이트와 패키지 매니저 업데이트가 겹치지 않도록 패키지 설치본에서는 인앱 업데이트를 "알림만"으로 두는 플래그(예: 환경 변수나 설치 경로 판별) 고려.
- **CHANGELOG.md**: 지금은 커밋 메시지와 릴리스 노트에만 변경 이력이 있다. 업데이트 대화상자에 "변경 사항 보기" 링크를 둘 때도 필요.
- 코드 서명: `scripts/build_release.ps1`에 `signtool` 경로가 이미 있다. 인증서가 없으면 SmartScreen 경고가 사용자 이탈의 큰 원인이므로, 무료 OSS 서명(예: SignPath Foundation) 신청을 검토.

---

## 6. 추천 진행 순서

1. **A2 → A1(1단계) → C3 테스트** — 작고 안전하며 오동작을 직접 줄인다.
2. **D1 README·FAQ** — 코드 변경 없이 사용자 혼란과 지원 문의를 줄인다.
3. **B1 마우스 필터** — 가장 수요가 큰 신규 기능. 기존 `Debouncer` 재사용으로 구현량이 생각보다 작다.
4. **B2 키별 UI → B3 자동 추천** — 진단 → 조치 흐름을 앱 안에서 완결.
5. **C1 워치독, B5 자동 비활성화, B4 단축키** — 상주 프로그램으로서의 신뢰성과 게임 사용자 대응.
6. **A1(2단계 Release defer), C2 상승 실행, D2 배포** — 효과는 있으나 설계·운영 비용이 큰 것.

---

## 참고 자료

- KeyboardChatterBlocker — https://github.com/FreneticLLC/KeyboardChatterBlocker (구 https://github.com/mcmonkeyprojects/KeyboardChatterBlocker)
- KeyboardChatteringFix-Linux — https://github.com/finkrer/KeyboardChatteringFix-Linux
- keyboard-unchatter — https://github.com/ZoserLock/keyboard-unchatter
- Unshaky (macOS) — https://github.com/aahung/Unshaky , https://9to5mac.com/2019/04/16/unshaky/
- Keyboard Chattering / Double Typing Fix 가이드 — https://mkbguide.com/blog/keyboard-chattering-double-typing-fix
- Digital Trends 소개 기사 — https://digitaltrends.com/?p=3695723
- QMK Debounce 알고리즘 — https://develop-docs.qmk.fm/feature_debounce_type
- Win32 `FILTERKEYS`(BounceKeys) — https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-filterkeys
- `SetWindowsHookEx` — https://learn.microsoft.com/nl-be/windows/win32/api/winuser/nf-winuser-setwindowshookexa
- UIAccess와 UIPI(WinSDK 블로그 보관본) — https://learn.microsoft.com/cs-cz/archive/blogs/winsdk/setwindowhookex-api-with-wh_journalrecord-this-api-fails-with-access-denied-error-5
- Steam 커뮤니티: 채터링 수정 도구와 VAC — https://steamcommunity.com/discussions/forum/9/1771511442685838955
- Razer Insider: 이중 입력 사례(KeyboardChatterBlocker #15 언급) — https://insider.razer.com/razer-support-45/blackwidow-v3-pro-double-keys-33576
