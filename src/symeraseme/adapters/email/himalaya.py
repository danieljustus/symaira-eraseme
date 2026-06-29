from __future__ import annotations

import json
import logging
import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from datetime import datetime
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate, make_msgid
from functools import lru_cache
from pathlib import Path
from typing import Any

from symeraseme.adapters.email._types import Envelope, Message, SmtpConfig

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class EmailError(Exception):
    """Base exception for all email sending errors."""


class HimalayaError(EmailError):
    """Raised when the Himalaya CLI subprocess fails."""


class HimalayaNotInstalledError(HimalayaError):
    """Raised when the Himalaya CLI is not found on PATH."""


class SmtpError(EmailError):
    """Raised when SMTP sending fails."""


# ---------------------------------------------------------------------------
# Version detection
# ---------------------------------------------------------------------------


@lru_cache(maxsize=1)
def _detect_himalaya_version() -> tuple[int, int, int]:
    """Detect Himalaya CLI version as (major, minor, patch).

    Returns (0, 0, 0) when the version cannot be parsed.
    """
    _check_himalaya_installed()
    try:
        result = subprocess.run(
            ["himalaya", "--version"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        # Output looks like: "himalaya 1.2.0" or "himalaya v1.2.0"
        match = re.search(r"(\d+)\.(\d+)\.(\d+)", result.stdout)
        if match:
            return (int(match.group(1)), int(match.group(2)), int(match.group(3)))
    except (subprocess.TimeoutExpired, OSError):
        pass
    return (0, 0, 0)


def _is_v1_plus() -> bool:
    """Return True if Himalaya CLI is v1.0 or later."""
    return _detect_himalaya_version() >= (1, 0, 0)


# ---------------------------------------------------------------------------
# Config parsing
# ---------------------------------------------------------------------------


def _config_path_for_account() -> Path:
    """Return the default Himalaya config file path."""
    return Path.home() / ".config" / "himalaya" / "config.toml"


@lru_cache(maxsize=4)
def _read_himalaya_account_email(account: str = "") -> str:
    """Read the email address for *account* from the Himalaya TOML config.

    When *account* is empty the first (or default) account is used.
    Returns an empty string when the config or email cannot be found.
    """
    config_file = _config_path_for_account()
    if not config_file.is_file():
        return ""

    try:
        import tomllib  # Python 3.11+
    except ModuleNotFoundError:
        try:
            import tomli as tomllib  # type: ignore[no-redef]
        except ModuleNotFoundError:
            logger.warning("Cannot parse TOML: no tomllib/tomli available")
            return ""

    try:
        with open(config_file, "rb") as fh:
            data = tomllib.load(fh)
    except OSError as exc:
        logger.warning("Cannot read Himalaya config %s: %s", config_file, exc)
        return ""

    accounts: dict[str, Any] = data.get("accounts", {})
    if not accounts:
        return ""

    if account and account in accounts:
        return str(accounts[account].get("email", ""))

    # Fall back to the first account
    first_key = next(iter(accounts))
    return str(accounts[first_key].get("email", ""))


# ---------------------------------------------------------------------------
# Core helpers
# ---------------------------------------------------------------------------


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
    """Run a Himalaya CLI command with automatic version-aware flag placement.

    For v1.x the ``--account`` flag is placed *after* the first subcommand
    (e.g. ``himalaya envelope list --account NAME``).
    For v0.x it is placed globally before the subcommand.
    """
    _check_himalaya_installed()

    cmd = ["himalaya"]
    if config_path:
        cmd.extend(["--config", str(config_path)])

    if _is_v1_plus():
        # v1.x: --account goes after the first subcommand element
        if args:
            cmd.append(args[0])
            if account:
                cmd.extend(["--account", account])
            cmd.extend(args[1:])
        else:
            if account:
                cmd.extend(["--account", account])
    else:
        # v0.x: --account is a global option
        if account:
            cmd.extend(["--account", account])
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


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


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
    if _is_v1_plus():
        subcmd = [
            "envelope",
            "list",
            "--folder",
            folder,
            "--page-size",
            str(page_size),
            "--page",
            str(page),
            "--output",
            "json",
        ]
    else:
        subcmd = [
            "list",
            "--folder",
            folder,
            "--page-size",
            str(page_size),
            "--page",
            str(page),
        ]

    result = _run_himalaya(
        subcmd,
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
    """Build a raw MIME message string from an EmailMessage.

    Returns (mime_string, message_id).
    """
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
            except (OSError, ValueError, RuntimeError) as e:
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
    except (OSError, ValueError, RuntimeError) as e:
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
    """Send an email via the Himalaya CLI.

    For v1.x, builds a MIME message and pipes it to ``himalaya message send``.
    For v0.x, uses the legacy ``himalaya send --to ... --subject ...`` flags.
    """
    _check_himalaya_installed()
    message_id = make_msgid()

    if _is_v1_plus():
        # --- v1.x: build MIME and pipe to ``himalaya message send`` ---
        from_addr = _read_himalaya_account_email(account or "")
        if not from_addr:
            msg = (
                "Cannot determine sender email from Himalaya config. "
                "Ensure ~/.config/himalaya/config.toml has an [accounts.*] "
                "section with an 'email' field, or pass the account name."
            )
            raise HimalayaError(msg)

        email_msg = EmailMessage(to=to, subject=subject, body=body, cc=cc, bcc=bcc)
        mime_text, _ = _build_mime(email_msg, from_addr)

        cmd = ["himalaya", "message", "send"]
        if account:
            cmd.extend(["--account", account])
        if config_path:
            cmd.extend(["--config", str(config_path)])

        logger.debug("Running: %s", " ".join(cmd))
        try:
            result = subprocess.run(
                cmd,
                input=mime_text,
                capture_output=True,
                text=True,
                timeout=30,
            )
        except subprocess.TimeoutExpired:
            msg = "Himalaya message send timed out"
            raise HimalayaError(msg) from None

        if result.returncode != 0:
            stderr = result.stderr.strip()
            msg = f"Himalaya message send failed (exit {result.returncode}): {stderr}"
            raise HimalayaError(msg)

        return {"result": result.stdout.strip(), "message_id": message_id}

    # --- v0.x: legacy flag-based send ---
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

    return {"result": result.stdout.strip(), "message_id": message_id}


def send_message_smtp(
    to: str,
    subject: str,
    body: str,
    *,
    cc: str | None = None,
    bcc: str | None = None,
    smtp_config: SmtpConfig | None = None,
) -> dict[str, str]:
    """Send a single email via SMTP (synchronous, stdlib smtplib).

    Falls back to environment variables via :func:`load_smtp_config` when
    *smtp_config* is ``None``.
    """
    import smtplib

    if smtp_config is None:
        smtp_config = load_smtp_config()

    from_addr = smtp_config.from_addr
    if not from_addr:
        raise SmtpError(
            "SYMERASEME_SMTP_FROM is not configured. Set it in your environment or .env file."
        )

    recipients = [to]
    if cc:
        recipients.append(cc)
    if bcc:
        recipients.append(bcc)

    mime_text, message_id = _build_mime(
        EmailMessage(to=to, subject=subject, body=body, cc=cc, bcc=bcc),
        from_addr,
    )

    try:
        with smtplib.SMTP(smtp_config.host, smtp_config.port, timeout=30) as smtp:
            if smtp_config.use_tls:
                smtp.starttls()
            if smtp_config.username and smtp_config.password:
                smtp.login(smtp_config.username, smtp_config.password)
            smtp.sendmail(from_addr, recipients, mime_text)
    except smtplib.SMTPException as e:
        raise SmtpError(str(e)) from e
    except OSError as e:
        raise SmtpError(str(e)) from e

    return {"result": "Message sent", "message_id": message_id}


def get_email_backend() -> str:
    """Return the configured email backend (``smtp`` or ``himalaya``).

    Reads the ``SYMERASEME_EMAIL_BACKEND`` environment variable.
    Defaults to ``smtp`` when unset.
    """
    backend = os.environ.get("SYMERASEME_EMAIL_BACKEND", "smtp").lower().strip()
    if backend not in ("smtp", "himalaya"):
        logger.warning(
            "Unknown SYMERASEME_EMAIL_BACKEND=%r, falling back to smtp",
            backend,
        )
        return "smtp"
    return backend


def send_email(
    to: str,
    subject: str,
    body: str,
    *,
    cc: str | None = None,
    bcc: str | None = None,
    account: str | None = None,
    config_path: str | Path | None = None,
    smtp_config: SmtpConfig | None = None,
    backend: str | None = None,
) -> dict[str, str]:
    """Send an email via the configured backend (SMTP by default).

    Dispatches to :func:`send_message_smtp` or :func:`send_message`
    (Himalaya CLI subprocess) based on *backend* or the
    ``SYMERASEME_EMAIL_BACKEND`` environment variable.

    Raises :class:`EmailError` (or subclass) on failure.
    """
    if backend is None:
        backend = get_email_backend()

    if backend == "himalaya":
        return send_message(
            to=to,
            subject=subject,
            body=body,
            cc=cc,
            bcc=bcc,
            account=account,
            config_path=config_path,
        )

    return send_message_smtp(
        to=to,
        subject=subject,
        body=body,
        cc=cc,
        bcc=bcc,
        smtp_config=smtp_config,
    )


def send_raw_email(
    to: str,
    raw_message: str,
    *,
    account: str | None = None,
    config_path: str | Path | None = None,
) -> str:
    """Send a raw MIME message via Himalaya CLI.

    For v1.x uses ``himalaya message send``; for v0.x uses ``himalaya send``.
    """
    _check_himalaya_installed()

    cmd = ["himalaya", "message", "send"] if _is_v1_plus() else ["himalaya", "send"]

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
    # v1.x: ``envelope get`` was removed; use ``message read`` instead.
    # v0.x: ``get`` is the legacy flat command.
    if _is_v1_plus():
        subcmd = ["message", "read", message_id, "--output", "json"]
    else:
        subcmd = ["get", message_id, "--json"]

    result = _run_himalaya(
        subcmd,
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
