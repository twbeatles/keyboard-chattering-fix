"""Unit tests for the release manifest scripts (no secrets required).

Covers builder input validation, verifier payload validation, asset
cross-checks, and the signed Python<->Rust interop fixture. Run from the
repository root: ``python -m pytest scripts/``.
"""

import hashlib
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import build_update_manifest as builder
import verify_update_manifest as verifier

# Signed with the real release private key (v99.0.0 fixture, expires 2036) --
# the same document the Rust test verifies against UPDATE_PUBLIC_KEY_B64.
SIGNED_FIXTURE = """{
    "payload": {
        "artifact_url": "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v99.0.0/KeyboardChatteringFix.exe",
        "expires_at": "2036-10-13T07:13:12Z",
        "sha256": "295b7aeb83e7f557398c922f6f302a9e2f63b107765fcf9ab4f11e7bb0cf9295",
        "size": 1024,
        "tag": "v99.0.0",
        "version": "99.0.0"
    },
    "signature": "YdRBDY39QhI5KexZrkqMW/Wtg0vX+dOI1u0VMIu8dRPa+iSVzO4zRRx/2q4qXB9pl62IWXDcd/ZuaYSjSmtGDQ=="
}"""


def _payload(**overrides):
    base = {
        "version": "0.2.0",
        "tag": "v0.2.0",
        "artifact_url": "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe",
        "sha256": "0" * 64,
        "size": 1024,
        "expires_at": "2036-01-01T00:00:00Z",
    }
    base.update(overrides)
    return base


def test_verifier_accepts_signed_fixture(tmp_path):
    manifest = tmp_path / "update.json"
    manifest.write_text(SIGNED_FIXTURE, encoding="utf-8")
    assert verifier.main(["--manifest", str(manifest)]) == 0


def test_verifier_rejects_tampered_signature(tmp_path):
    from cryptography.exceptions import InvalidSignature

    manifest = tmp_path / "update.json"
    manifest.write_text(SIGNED_FIXTURE.replace("99.0.0", "99.0.1"), encoding="utf-8")
    with pytest.raises(InvalidSignature):
        verifier.main(["--manifest", str(manifest)])


def test_validate_payload_rejects_bad_shapes():
    with pytest.raises(ValueError, match="fields"):
        verifier.validate_payload({k: v for k, v in _payload().items() if k != "size"})
    with pytest.raises(ValueError, match="tag"):
        verifier.validate_payload(_payload(tag="v9.9.9"))
    with pytest.raises(ValueError, match="pinned"):
        verifier.validate_payload(_payload(artifact_url="https://evil.example.com/a.exe"))
    # UG5: any other repository or asset name under github.com is rejected
    # too, exactly like the app's pin.
    for url in (
        "https://github.com/someone-else/keyboard-chattering-fix/releases/download/v0.2.0/KeyboardChatteringFix.exe",
        "https://github.com/twbeatles/keyboard-chattering-fix/releases/download/v0.2.0/kcfix-updater.exe",
    ):
        with pytest.raises(ValueError, match="pinned"):
            verifier.validate_payload(_payload(artifact_url=url))
    with pytest.raises(ValueError, match="sha256"):
        verifier.validate_payload(_payload(sha256="xyz"))
    for bad_size in (0, -5, True, 512 * 1024 * 1024 + 1, "1024"):
        with pytest.raises(ValueError, match="size"):
            verifier.validate_payload(_payload(size=bad_size))


def test_validate_payload_expiry_is_fail_closed():
    with pytest.raises(ValueError, match="expired"):
        verifier.validate_payload(_payload(expires_at="2000-01-01T00:00:00Z"))
    with pytest.raises(ValueError, match="not a valid timestamp"):
        verifier.validate_payload(_payload(expires_at="soon"))
    # Missing timestamp: the script requires the full field set (stricter than
    # the app, which tolerates pre-expiry legacy manifests).
    payload = _payload()
    del payload["expires_at"]
    with pytest.raises(ValueError, match="fields"):
        verifier.validate_payload(payload)


def test_check_asset_cross_checks_bytes():
    blob = b"fake-binary-bytes"
    payload = _payload(size=len(blob), sha256=hashlib.sha256(blob).hexdigest())
    verifier.check_asset(payload, blob)
    with pytest.raises(ValueError, match="size"):
        verifier.check_asset(payload, blob + b"x")
    with pytest.raises(ValueError, match="hash"):
        verifier.check_asset(payload, b"X" * len(blob))


def test_builder_rejects_bad_inputs(tmp_path, monkeypatch):
    monkeypatch.delenv("KCFIX_UPDATE_PRIVATE_KEY_B64", raising=False)
    exe = tmp_path / "app.exe"
    exe.write_bytes(b"MZ")
    base = ["--artifact", str(exe), "--artifact-url", "https://github.com/o/r/releases/download/v1/a.exe"]
    with pytest.raises(ValueError, match="--tag"):
        builder.main(["--version", "v1.0.0", "--tag", "v2.0.0", *base, "--output", "o.json"])
    with pytest.raises(ValueError, match="HTTPS"):
        builder.main(["--version", "v1.0.0", "--tag", "v1.0.0", "--artifact", str(exe),
                      "--artifact-url", "http://insecure/x.exe", "--output", "o.json"])
    with pytest.raises(ValueError, match=".exe"):
        builder.main(["--version", "v1.0.0", "--tag", "v1.0.0", "--artifact", str(tmp_path / "missing.exe"),
                      "--artifact-url", "https://github.com/o/r/x.exe", "--output", "o.json"])
    with pytest.raises(ValueError, match="positive"):
        builder.main(["--version", "v1.0.0", "--tag", "v1.0.0", *base, "--output", "o.json",
                      "--expires-in-days", "0"])
    with pytest.raises(ValueError, match="PRIVATE_KEY"):
        builder.main(["--version", "v1.0.0", "--tag", "v1.0.0", *base, "--output", "o.json"])


def test_canonical_payload_is_compact_sorted_utf8():
    blob = builder.canonical_payload({"b": "가", "a": [1, 2]})
    assert blob == '{"a":[1,2],"b":"가"}'.encode("utf-8")
