"""Consent token mechanism for destructive operations.

Destructive commands (execute, reply with send) require explicit consent
via either:
- Interactive TTY prompt (`--yes` flag)
- A pre-issued consent token via `OPENERASEME_CONSENT` env var or `--consent` flag
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import time
from pathlib import Path

CONSENT_DIR = "~/.local/share/openeraseme"
TOKEN_TTL = 86400  # 24 hours in seconds


def _consent_dir() -> Path:
    d = Path(os.environ.get("OPENERASEME_DATA_DIR", CONSENT_DIR)).expanduser()
    d.mkdir(parents=True, exist_ok=True)
    return d


def issue_token(command: str, ttl: int = TOKEN_TTL) -> str:
    issued_at = int(time.time())
    expires_at = issued_at + ttl
    payload = json.dumps(
        {"command": command, "issued_at": issued_at, "expires_at": expires_at},
        sort_keys=True,
    )
    token = hmac.new(
        str(os.urandom(32)).encode(),
        payload.encode(),
        hashlib.sha256,
    ).hexdigest()[:16]

    token_file = _consent_dir() / f"consent_{token}.json"
    with open(token_file, "w") as f:
        f.write(payload)
    return token


def verify_token(command: str, token: str) -> bool:
    token_file = _consent_dir() / f"consent_{token}.json"
    if not token_file.exists():
        return False
    try:
        with open(token_file) as f:
            payload = json.load(f)
    except (json.JSONDecodeError, OSError):
        return False

    if payload.get("command") != command:
        return False

    expires_at = payload.get("expires_at", 0)
    if int(time.time()) > expires_at:
        token_file.unlink(missing_ok=True)
        return False

    return True


def consume_token(token: str) -> None:
    token_file = _consent_dir() / f"consent_{token}.json"
    token_file.unlink(missing_ok=True)


def check_consent(
    command: str,
    yes: bool = False,
    consent_token: str | None = None,
) -> bool:
    if yes:
        return True
    if consent_token:
        return verify_token(command, consent_token)
    env_token = os.environ.get("OPENERASEME_CONSENT", "")
    if env_token:
        return verify_token(command, env_token)
    return False
