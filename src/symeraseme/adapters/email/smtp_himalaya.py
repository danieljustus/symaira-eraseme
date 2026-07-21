"""SMTP transport shim for sending emails via stdlib smtplib."""

from __future__ import annotations

import logging
import smtplib
from dataclasses import dataclass
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate, make_msgid

from symeraseme.adapters.email._types import SmtpConfig
from symeraseme.adapters.email.himalaya_config import load_smtp_config

logger = logging.getLogger(__name__)


class SmtpError(Exception):
    """Raised when SMTP sending fails."""


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
