"""Consent token mechanism for destructive operations.

Destructive commands (execute, reply with send) require explicit consent
via either:
- Interactive TTY prompt (`--yes` flag)
- A pre-issued consent token via `SYMERASEME_CONSENT` env var or `--consent` flag
"""

from __future__ import annotations

import json
import os
import secrets
import sys
import time
from pathlib import Path

CONSENT_DIR = "~/.local/share/symeraseme"
TOKEN_TTL = 86400  # 24 hours in seconds


def _consent_dir() -> Path:
    d = Path(os.environ.get("SYMERASEME_DATA_DIR", CONSENT_DIR)).expanduser()
    d.mkdir(parents=True, exist_ok=True)
    return d


def issue_token(command: str, ttl: int = TOKEN_TTL) -> str:
    issued_at = int(time.time())
    expires_at = issued_at + ttl
    token = secrets.token_urlsafe(16)
    payload = json.dumps(
        {
            "command": command,
            "issued_at": issued_at,
            "expires_at": expires_at,
            "token": token,
        },
        sort_keys=True,
    )
    token_file = _consent_dir() / f"consent_{token}.json"
    with open(token_file, "w") as f:
        f.write(payload)
    os.chmod(token_file, 0o600)
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

    # New format: the token is stored in the payload — verify it matches
    # so that knowing only the filename is insufficient to verify.
    stored_token = payload.get("token")
    if stored_token is not None and stored_token != token:
        return False
    # Old format (backward compat): no stored token field; proceed with
    # existing checks (file existence + command + expiry).

    if payload.get("command") != command:
        return False

    expires_at = payload.get("expires_at", 0)
    if int(time.time()) > expires_at:
        token_file.unlink(missing_ok=True)
        return False

    # Ensure restrictive permissions on existing token files
    try:
        if token_file.stat().st_mode & 0o777 != 0o600:
            os.chmod(token_file, 0o600)
    except OSError:
        pass

    return True


def consume_token(token: str) -> None:
    token_file = _consent_dir() / f"consent_{token}.json"
    token_file.unlink(missing_ok=True)


def revoke_token(token: str) -> bool:
    """Revoke a consent token by removing its file.

    Returns True if the token existed and was revoked, False otherwise.
    """
    token_file = _consent_dir() / f"consent_{token}.json"
    if not token_file.exists():
        return False
    token_file.unlink(missing_ok=True)
    return True


def list_tokens() -> list[dict]:
    """List all active consent tokens with their metadata."""
    tokens: list[dict] = []
    now = int(time.time())
    consent_dir = _consent_dir()
    if not consent_dir.exists():
        return tokens
    for f in sorted(consent_dir.glob("consent_*.json")):
        try:
            payload = json.loads(f.read_text())
        except (json.JSONDecodeError, OSError):
            continue
        expiry = payload.get("expires_at", 0)
        if now > expiry:
            f.unlink(missing_ok=True)
            continue
        token_id = f.stem.replace("consent_", "")
        # Ensure restrictive permissions on existing token files
        try:
            if f.stat().st_mode & 0o777 != 0o600:
                os.chmod(f, 0o600)
        except OSError:
            pass
        tokens.append(
            {
                "token": token_id,
                "command": payload.get("command", "?"),
                "issued_at": payload.get("issued_at", 0),
                "expires_at": expiry,
            }
        )
    return tokens


def tty_available() -> bool:
    """Check whether an interactive TTY is available for prompting."""
    return sys.stdin.isatty() and sys.stdout.isatty()


def _tty_prompt(message: str = "Are you sure?") -> bool:
    """Prompt the user on the terminal, returning True on affirmative."""
    if not tty_available():
        return False
    try:
        response = input(f"{message} [y/N] ")
        return response.strip().lower() in ("y", "yes")
    except (EOFError, KeyboardInterrupt):
        return False


def check_consent(
    command: str,
    yes: bool = False,
    consent_token: str | None = None,
    interactive: bool = True,
) -> bool:
    if yes:
        return True
    if consent_token:
        return verify_token(command, consent_token)
    env_token = os.environ.get("SYMERASEME_CONSENT", "")
    if env_token:
        return verify_token(command, env_token)
    if interactive:
        return _tty_prompt(f"Destructive command '{command}' requires consent. Proceed?")
    return False
