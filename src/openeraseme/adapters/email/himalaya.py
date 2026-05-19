from __future__ import annotations

import json
import logging
import shutil
import subprocess
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class HimalayaError(Exception):
    pass


class HimalayaNotInstalledError(HimalayaError):
    pass


@dataclass
class Envelope:
    id: str
    subject: str
    from_: str
    to: str
    date: datetime | None = None
    flags: list[str] = field(default_factory=list)


@dataclass
class Message:
    id: str
    subject: str
    from_: str
    to: str
    date: datetime | None = None
    body: str = ""
    flags: list[str] = field(default_factory=list)


def _check_himalaya_installed() -> str:
    path = shutil.which("himalaya")
    if path is None:
        msg = (
            "Himalaya CLI is not installed. "
            "Install it via: cargo install himalaya "
            "or: brew install himalaya"
        )
        raise HimalayaNotInstalledError(msg)
    return path


def _run_himalaya(
    args: list[str],
    *,
    account: str | None = None,
    config_path: str | Path | None = None,
    timeout: int = 30,
) -> subprocess.CompletedProcess:
    _check_himalaya_installed()

    cmd = ["himalaya"]
    if account:
        cmd.extend(["--account", account])
    if config_path:
        cmd.extend(["--config", str(config_path)])

    cmd.extend(args)

    logger.debug("Running: %s", " ".join(cmd))
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        msg = f"Himalaya command timed out after {timeout}s: {' '.join(cmd)}"
        raise HimalayaError(msg) from None

    if result.returncode != 0:
        stderr = result.stderr.strip()
        msg = f"Himalaya command failed (exit {result.returncode}): {stderr}"
        raise HimalayaError(msg)

    return result


def hismalaya_available() -> bool:
    return shutil.which("himalaya") is not None


def list_messages(
    folder: str = "INBOX",
    page_size: int = 20,
    page: int = 1,
    *,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> list[Envelope]:
    result = _run_himalaya(
        ["list", "--folder", folder, "--page-size", str(page_size), "--page", str(page)],
        account=account,
        config_path=config_path,
    )

    if not result.stdout.strip():
        return []

    try:
        raw = json.loads(result.stdout)
    except json.JSONDecodeError:
        msg = f"Failed to parse Himalaya JSON output: {result.stdout[:500]}"
        raise HimalayaError(msg) from None

    envelopes: list[Envelope] = []
    for item in raw:
        env = Envelope(
            id=str(item.get("id", "")),
            subject=item.get("subject", ""),
            from_=_extract_address(item.get("from")),
            to=_extract_address(item.get("to")),
            date=_parse_date(item.get("date")),
            flags=item.get("flags", []),
        )
        envelopes.append(env)

    return envelopes


def send_message(
    to: str,
    subject: str,
    body: str,
    *,
    cc: str | None = None,
    bcc: str | None = None,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> str:
    _check_himalaya_installed()
    cmd = ["himalaya"]
    if account:
        cmd.extend(["--account", account])
    if config_path:
        cmd.extend(["--config", str(config_path)])
    cmd.extend(["send", "--to", to, "--subject", subject])
    if cc:
        cmd.extend(["--cc", cc])
    if bcc:
        cmd.extend(["--bcc", bcc])

    logger.debug("Running: %s", " ".join(cmd))
    try:
        result = subprocess.run(
            cmd,
            input=body,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        msg = "Himalaya send timed out"
        raise HimalayaError(msg) from None

    if result.returncode != 0:
        stderr = result.stderr.strip()
        msg = f"Himalaya send failed (exit {result.returncode}): {stderr}"
        raise HimalayaError(msg)

    return result.stdout.strip()


def send_raw_email(
    to: str,
    raw_message: str,
    *,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> str:
    _check_himalaya_installed()
    cmd = ["himalaya", "send"]
    if account:
        cmd.extend(["--account", account])
    if config_path:
        cmd.extend(["--config", str(config_path)])

    result = subprocess.run(
        cmd,
        input=raw_message,
        capture_output=True,
        text=True,
        timeout=30,
    )

    if result.returncode != 0:
        msg = f"Himalaya send failed (exit {result.returncode}): {result.stderr.strip()}"
        raise HimalayaError(msg)

    return result.stdout.strip()


def get_message(
    message_id: str,
    *,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> Message:
    result = _run_himalaya(
        ["get", message_id, "--json"],
        account=account,
        config_path=config_path,
    )

    if not result.stdout.strip():
        msg = f"Message {message_id} not found"
        raise HimalayaError(msg)

    try:
        raw = json.loads(result.stdout)
    except json.JSONDecodeError:
        msg = f"Failed to parse Himalaya JSON output: {result.stdout[:500]}"
        raise HimalayaError(msg) from None

    return Message(
        id=str(raw.get("id", message_id)),
        subject=raw.get("subject", ""),
        from_=_extract_address(raw.get("from")),
        to=_extract_address(raw.get("to")),
        date=_parse_date(raw.get("date")),
        body=raw.get("body", ""),
        flags=raw.get("flags", []),
    )


def _extract_address(field: Any) -> str:
    if isinstance(field, dict):
        return field.get("name", "") or field.get("addr", "")
    return str(field or "")


def _parse_date(value: Any) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, str):
        for fmt in (
            "%Y-%m-%dT%H:%M:%S%z",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S %z",
            "%a, %d %b %Y %H:%M:%S %z",
        ):
            try:
                return datetime.strptime(value, fmt)
            except ValueError:
                continue
    return None
