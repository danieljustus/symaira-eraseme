from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from datetime import datetime
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate, make_msgid
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


def himalaya_available() -> bool:
    return shutil.which("himalaya") is not None


def hismalaya_available() -> bool:
    """Deprecated: use himalaya_available instead."""
    import warnings

    warnings.warn(
        "hismalaya_available is deprecated; use himalaya_available instead.",
        DeprecationWarning,
        stacklevel=2,
    )
    return himalaya_available()


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


@dataclass
class SmtpConfig:
    host: str = "localhost"
    port: int = 587
    username: str = ""
    password: str = ""
    use_tls: bool = True
    from_addr: str = ""


def load_smtp_config() -> SmtpConfig:
    """Load SMTP configuration from environment variables.

    Reads:
        SYMERASEME_SMTP_HOST       (default: localhost)
        SYMERASEME_SMTP_PORT       (default: 587)
        SYMERASEME_SMTP_USER       (default: "")
        SYMERASEME_SMTP_PASSWORD   (default: "")
        SYMERASEME_SMTP_TLS        (default: 1)
        SYMERASEME_SMTP_FROM       (default: "")
    """
    return SmtpConfig(
        host=os.environ.get("SYMERASEME_SMTP_HOST", "localhost"),
        port=int(os.environ.get("SYMERASEME_SMTP_PORT", "587")),
        username=os.environ.get("SYMERASEME_SMTP_USER", ""),
        password=os.environ.get("SYMERASEME_SMTP_PASSWORD", ""),
        use_tls=os.environ.get("SYMERASEME_SMTP_TLS", "1").lower() in ("1", "true", "yes"),
        from_addr=os.environ.get("SYMERASEME_SMTP_FROM", ""),
    )


@dataclass
class EmailMessage:
    to: str
    subject: str
    body: str
    cc: str | None = None
    bcc: str | None = None


def _build_mime(msg: EmailMessage, from_addr: str) -> tuple[str, str]:
    mime = MIMEMultipart("mixed")
    mime["From"] = from_addr
    mime["To"] = msg.to
    mime["Subject"] = msg.subject
    mime["Date"] = formatdate(localtime=True)
    if msg.cc:
        mime["Cc"] = msg.cc

    message_id = make_msgid()
    mime["Message-ID"] = message_id

    part = MIMEText(msg.body, "plain")
    mime.attach(part)

    return mime.as_string(), message_id


async def send_messages_batch(
    messages: list[EmailMessage],
    *,
    smtp_config: SmtpConfig | None = None,
) -> list[dict[str, Any]]:
    """Send multiple emails over a single SMTP connection.

    Opens one SMTP connection, sends all messages over it,
    then closes. Failed sends are collected per-message without
    aborting the batch.

    Parameters
    ----------
    messages : list[EmailMessage]
        The email messages to send.
    smtp_config : SmtpConfig | None
        SMTP connection parameters. Falls back to env vars when ``None``.

    Returns
    -------
    list[dict[str, Any]]
        One dict per input message with keys ``success``, ``to``,
        ``subject``, and optionally ``error``.
    """
    import aiosmtplib

    if smtp_config is None:
        smtp_config = load_smtp_config()

    from_addr = smtp_config.from_addr

    results: list[dict[str, Any]] = []
    if not messages:
        return results

    try:
        smtp = aiosmtplib.SMTP(
            hostname=smtp_config.host,
            port=smtp_config.port,
            timeout=30,
        )

        await smtp.connect()

        if smtp_config.use_tls:
            await smtp.starttls()

        if smtp_config.username and smtp_config.password:
            await smtp.login(smtp_config.username, smtp_config.password)

        for msg in messages:
            try:
                mime_text, message_id = _build_mime(msg, from_addr)
                recipients = [msg.to]
                if msg.cc:
                    recipients.append(msg.cc)
                if msg.bcc:
                    recipients.append(msg.bcc)

                await smtp.sendmail(from_addr, recipients, mime_text)
                results.append(
                    {
                        "success": True,
                        "to": msg.to,
                        "subject": msg.subject,
                        "message_id": message_id,
                    }
                )
            except Exception as e:
                logger.warning("Failed to send to %s: %s", msg.to, e)
                results.append(
                    {
                        "success": False,
                        "to": msg.to,
                        "subject": msg.subject,
                        "error": str(e),
                    }
                )

        await smtp.quit()
    except Exception as e:
        logger.error("SMTP connection failed: %s", e)
        for msg in messages:
            results.append(
                {
                    "success": False,
                    "to": msg.to,
                    "subject": msg.subject,
                    "error": str(e),
                }
            )

    return results


def send_message(
    to: str,
    subject: str,
    body: str,
    *,
    cc: str | None = None,
    bcc: str | None = None,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> dict[str, str]:
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
    message_id = make_msgid()
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

    return {"result": result.stdout.strip(), "message_id": message_id}


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
    from symeraseme.core.datetime_utils import parse_iso_datetime

    return parse_iso_datetime(value)
