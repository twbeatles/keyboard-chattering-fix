"""Verify a signed update manifest against the public key embedded in the app."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

ROOT = Path(__file__).resolve().parents[1]
CONFIG_RS = ROOT / "src" / "config.rs"


def _read_embedded_pubkey() -> str:
    text = CONFIG_RS.read_text(encoding="utf-8")
    match = re.search(r'pub const UPDATE_PUBLIC_KEY_B64:\s*&str\s*=\s*"([^"]+)"', text)
    if match is None:
        raise ValueError(f"UPDATE_PUBLIC_KEY_B64 not found in {CONFIG_RS}")
    return match.group(1)


UPDATE_PUBLIC_KEY_B64 = _read_embedded_pubkey()


def _canonical_payload(payload: dict[str, object]) -> bytes:
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


MAX_ARTIFACT_BYTES = 512 * 1024 * 1024


def _parse_expiry(value: object) -> "datetime | None":
    """Parse an RFC3339 timestamp. Returns None when the field is absent."""
    if value is None:
        return None
    text = str(value).strip()
    try:
        return datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError:
        pass
    for fmt in ("%Y-%m-%dT%H:%M:%S%z",):
        try:
            return datetime.strptime(text, fmt).astimezone(timezone.utc)
        except ValueError:
            continue
    try:
        return datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        raise ValueError(f"expires_at is not a valid timestamp: {value!r}")


def validate_payload(payload: dict[str, object]) -> None:
    """Mirror the app-side checks (fail closed, like the Rust verifier)."""
    required = {"version", "tag", "artifact_url", "sha256", "size", "expires_at"}
    if set(payload) != required:
        raise ValueError("Manifest payload fields are invalid")
    if payload["tag"] != f"v{payload['version']}":
        raise ValueError("Manifest tag does not match version")
    if not str(payload["artifact_url"]).startswith("https://github.com/"):
        raise ValueError("Manifest artifact URL is not pinned to GitHub releases")
    sha256 = str(payload["sha256"])
    if len(sha256) != 64 or any(c not in "0123456789abcdefABCDEF" for c in sha256):
        raise ValueError("Manifest sha256 is invalid")
    size = payload["size"]
    if not isinstance(size, int) or isinstance(size, bool) or not 0 < size <= MAX_ARTIFACT_BYTES:
        raise ValueError("Manifest size is out of range")
    expires = _parse_expiry(payload.get("expires_at"))
    if expires is not None and expires <= datetime.now(timezone.utc):
        raise ValueError("Manifest has expired")


def check_asset(payload: dict[str, object], blob: bytes) -> None:
    """Cross-check a published binary against the manifest (R-001)."""
    if len(blob) != payload["size"]:
        raise ValueError(f"Asset size {len(blob)} does not match manifest size {payload['size']}")
    actual = hashlib.sha256(blob).hexdigest()
    if actual.lower() != str(payload["sha256"]).lower():
        raise ValueError(f"Asset hash {actual} does not match manifest")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify signed update.json")
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--expect-asset", type=Path, default=None, help="Verify a downloaded binary against the manifest hash/size (R-001)")
    args = parser.parse_args(argv)
    document = json.loads(args.manifest.read_text(encoding="utf-8"))
    payload = dict(document["payload"])
    signature = base64.b64decode(str(document["signature"]), validate=True)
    public = base64.b64decode(UPDATE_PUBLIC_KEY_B64, validate=True)
    Ed25519PublicKey.from_public_bytes(public).verify(signature, _canonical_payload(payload))
    validate_payload(payload)
    if args.expect_asset is not None:
        check_asset(payload, args.expect_asset.read_bytes())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
