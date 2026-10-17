# 업데이트 배포 (GitHub Releases)

이 문서는 `Keyboard Chatter Fix`의 자동 업데이트 파이프라인을 설명합니다.
구조는 `kakaotalk-pc-adblock-py` 저장소의 검증된 업데이터와 동일합니다:
서명된 매니페스트(`update.json`) + 앱 내 확인/스테이징 + 별도 헬퍼
(`kcfix-updater.exe`)가 실제 파일 교체를 수행합니다.

## 동작 방식

1. 앱(트레이 메뉴 **업데이트 확인** 또는 `KeyboardChatteringFix.exe --check-update`)이
   `https://github.com/twbeatles/keyboard-chattering-fix/releases/latest/download/update.json`
   을 내려받습니다.
2. 매니페스트의 Ed25519 서명을 `src/config.rs`의 `UPDATE_PUBLIC_KEY_B64`로
   검증하고, 아티팩트 URL이 이 저장소의 Releases 경로와 정확히 일치하는지,
   버전이 현재보다 새 버전인지 확인합니다.
3. 새 EXE를 SHA-256으로 검증하면서 `%TEMP%`에 내려받고, 같은 폴더의
   `kcfix-updater.exe`를 스테이징한 뒤 헬퍼를 실행하고 앱이 종료됩니다.
4. 헬퍼는 앱 종료를 기다렸다가 스테이징된 파일을 교체 직전 **다시**
   해시 검증하고, `KeyboardChatteringFix.exe`를 교체한 뒤 새 버전을 실행합니다.
   새 버전이 60초 안에 살아 있음을 증명하지 못하면 자동으로 롤백합니다.
5. 교체 실패 시 이전 버전을 다시 실행하고 오류 대화상자를 보여줍니다.
   (단, 앱이 아직 살아 있는 대기-초과 상황에서는 중복 실행하지 않습니다.)

## 릴리즈 절차

1. 두 곳의 버전을 함께 올립니다 (반드시 `major.minor.patch` 3 숫자):
   - `Cargo.toml`의 `[package] version`
   - `src/config.rs`의 `pub const VERSION`
   - `cargo test --workspace`의 `version_sync` 테스트가 불일치를 차단합니다.
2. 커밋 후 태그를 찍고 푸시합니다:
   ```powershell
   git tag v0.2.0
   git push origin v0.2.0
   ```
   태그는 반드시 `v{VERSION}`이어야 합니다. `release.yml`이 이를 검증하고,
   불일치하면 릴리즈가 중단됩니다. 워크플로는 `vX.Y.Z` 형태의 태그에만
   반응하므로(`v*` 오타 태그는 실행되지 않음), 태그 이름 오타에 주의하세요.
3. GitHub Actions(`release`)가 자동으로 테스트 → 빌드 → 서명된 `update.json`
   생성 → Release 에셋 업로드(`KeyboardChatteringFix.exe`,
   `kcfix-updater.exe`, `KeyboardChatteringFix.zip`, `update.json`) →
   게시된 매니페스트 재검증을 수행합니다.

사용자는 설치 폴더에 두 EXE를 함께 두기만 하면 됩니다.
헬퍼가 없으면 업데이트 확인 시 그 취지의 안내가 나옵니다.

## 최초 1회 설정 (서명 비밀키)

매니페스트 서명용 Ed25519 키페어는 이미 생성되어 있습니다.

- 앱에 내장된 공개키: `src/config.rs`의 `UPDATE_PUBLIC_KEY_B64`
- 비공개키는 GitHub 저장소의 Actions secret으로 등록하세요:
  - Name: `KCFIX_UPDATE_PRIVATE_KEY_B64`
  - Value: (최초 생성 시 전달된 base64 44자리 문자열)
  - Settings → Secrets and variables → Actions → New repository secret

비공개키는 저장소에 절대 커밋하지 마세요.
(`scripts/build_update_manifest.py`는 환경변수에서만 읽습니다.)

## 코드 사이닝 (선택, Authenticode)

릴리스 바이너리의 Authenticode 서명은 선택 사항입니다. 무결성은 Ed25519
매니페스트가 담당하므로, 서명이 없어도 업데이트 검증에는 지장이 없습니다
(SmartScreen 평판에만 영향). 서명하려면 다음 중 하나를 Actions secret으로
등록하세요. 둘 다 없으면 릴리스는 기존처럼 미서명으로 게시됩니다.

- PFX 파일 방식: `SIGN_PFX_PATH` (파일 경로), `SIGN_PFX_PASSWORD` (선택)
- 인증서 저장소 방식: `SIGN_CERT_SHA1` (지문), `SIGN_CERT_STORE`·`SIGN_CERT_SUBJECT` (선택)
- 타임스탬프 서버 변경 시: `SIGN_TIMESTAMP_URL` (기본값 `http://timestamp.digicert.com`)

## 키 교체 시

1. 새 키페어를 생성합니다:
   ```powershell
   python -c "import base64; from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey; k=Ed25519PrivateKey.generate(); print('PRIV:'+base64.b64encode(k.private_bytes_raw()).decode()); print('PUB:'+base64.b64encode(k.public_key().public_bytes_raw()).decode())"
   ```
2. `src/config.rs`의 `UPDATE_PUBLIC_KEY_B64`를 새 공개키로 교체합니다.
3. GitHub secret `KCFIX_UPDATE_PRIVATE_KEY_B64`를 새 비공개키로 교체합니다.
4. 새 버전 릴리즈 후부터 새 키로 서명한 매니페스트만 인정됩니다.
   (이전 버전 앱은 이전 키로 서명한 매니페스트만 인정하므로,
   키 교체 자체는 반드시 새 버전 릴리즈와 함께 배포됩니다.)

## 로컬 검증

```powershell
# 전체 테스트 (버전 동기화 + 업데이터 단위 테스트 포함)
cargo test --workspace

# 릴리즈 빌드 + 스모크 (서명 없음)
powershell -ExecutionPolicy Bypass -File .\scripts\build_release.ps1 -NoSign
```

## 매니페스트 형식

`scripts/build_update_manifest.py`가 생성하는 `update.json`:

```json
{
  "payload": {
    "version": "0.2.0",
    "tag": "v0.2.0",
    "artifact_url": "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe",
    "sha256": "<64자리 hex>",
    "size": 1234567,
    "expires_at": "2027-…Z"
  },
  "signature": "<Ed25519 서명 base64>"
}
```

서명은 `payload`를 `json.dumps(ensure_ascii=False, sort_keys=True,
separators=(",", ":"))` 정준 형태로 직렬화한 바이트에 대한 것입니다.
(Rust 측 `updater::canonical::canonical_payload_python`과 동일)

## 저장소 이름을 바꾸거나 포크하는 경우

두 상수를 함께 바꾸세요 (한 곳만 바꾸면 업데이트가 멈춥니다):

- `src/updater/model.rs`: `MANIFEST_URL`, `RELEASE_DOWNLOAD_PREFIX`
- `src/updater/manifest.rs`의 테스트 기대 URL이 함께 검증해 줍니다.
