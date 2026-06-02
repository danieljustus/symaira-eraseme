"""IMAP polling and SMTP fallback for broker reply handling."""

from __future__ import annotations

import contextlib
import email
import email.utils
import imaplib
import logging
import re
from datetime import UTC, datetime, timedelta
from email.header import decode_header
from typing import Any

from symeraseme.adapters.email._types import Envelope, Message

logger = logging.getLogger(__name__)

# Common reply markers in subject lines
RE_PREFIX = re.compile(
    r"^(Re|Fwd|Aw|Antwort|R\xe9f\.|SV|VS|WG|AW|RE|REF)\s*:\s*",
    re.IGNORECASE,
)
# Reference-based tracking via In-Reply-To / References headers
RE_MESSAGE_ID = re.compile(r"<[^>]+>")


class IMAPError(Exception):
    """IMAP error."""

    pass


def decode_mime_header(value: str | None) -> str:
    if not value:
        return ""
    decoded_parts = decode_header(value)
    parts: list[str] = []
    for data, charset in decoded_parts:
        if isinstance(data, bytes):
            try:
                parts.append(data.decode(charset or "utf-8", errors="replace"))
            except LookupError:
                parts.append(data.decode("utf-8", errors="replace"))
        else:
            parts.append(str(data))
    return " ".join(parts)


def extract_thread_id(headers: dict[str, Any]) -> str | None:
    """Extract a thread identifier from email headers.

    Tries References, In-Reply-To, or falls back to Message-ID.
    """
    refs = headers.get("References", "")
    if refs:
        found = RE_MESSAGE_ID.findall(refs)
        if found:
            return found[0]

    in_reply = headers.get("In-Reply-To", "")
    if in_reply:
        m = RE_MESSAGE_ID.search(in_reply)
        if m:
            return m[0]

    msg_id = headers.get("Message-ID", "")
    if msg_id:
        m = RE_MESSAGE_ID.search(msg_id)
        if m:
            return m[0]

    return None


def normalize_subject(subject: str) -> str:
    """Strip reply prefixes from a subject to get the base thread subject."""
    cleaned = subject.strip()
    while True:
        match = RE_PREFIX.match(cleaned)
        if match:
            cleaned = cleaned[match.end() :].strip()
        else:
            break
    return cleaned


def subject_matches(base_subject: str, reply_subject: str) -> bool:
    """Check if a reply subject belongs to the same thread as a base subject."""
    return normalize_subject(reply_subject).lower() == normalize_subject(base_subject).lower()


def parse_email_body(raw_body: str, max_length: int = 500) -> str:
    """Extract plain text from email body, truncated."""
    if not raw_body:
        return ""
    text = raw_body.strip()
    if len(text) > max_length:
        text = text[:max_length] + "..."
    return text


def _parse_email(raw_message: bytes) -> dict[str, Any]:
    """Parse a raw RFC822 email into a structured dict."""
    msg = email.message_from_bytes(raw_message)
    headers: dict[str, Any] = {}

    for key in ("Subject", "From", "To", "Date", "Message-ID", "In-Reply-To", "References"):
        value = msg.get(key)
        if value:
            headers[key] = decode_mime_header(value)

    body = ""
    if msg.is_multipart():
        for part in msg.walk():
            if part.get_content_type() == "text/plain":
                payload = part.get_payload(decode=True)
                if isinstance(payload, bytes):
                    body = payload.decode("utf-8", errors="replace")
                break
    else:
        payload = msg.get_payload(decode=True)
        if isinstance(payload, bytes):
            body = payload.decode("utf-8", errors="replace")

    return {
        "headers": headers,
        "body": body,
        "message_id": headers.get("Message-ID", ""),
        "thread_id": extract_thread_id(headers),
        "from_addr": headers.get("From", ""),
        "subject": headers.get("Subject", ""),
    }


def poll_inbox(
    *,
    host: str = "imap.gmail.com",
    port: int = 993,
    username: str = "",
    password: str = "",
    ssl: bool = True,
    folder: str = "INBOX",
    since_days: int = 1,
    max_messages: int = 50,
) -> list[dict[str, Any]]:
    """Poll IMAP inbox for new messages.

    Returns parsed messages with headers and body.
    """
    try:
        mail = imaplib.IMAP4_SSL(host, port) if ssl else imaplib.IMAP4(host, port)
    except (OSError, imaplib.IMAP4.error) as e:
        logger.debug("Failed to connect to %s:%s: %s", host, port, e)
        msg = f"Failed to connect to mail server: {e}"
        raise IMAPError(msg) from e

    try:
        mail.login(username, password)
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        logger.debug("IMAP login failed for %s: %s", username, e)
        msg = f"IMAP login failed: {e}"
        raise IMAPError(msg) from e

    try:
        mail.select(folder)
        since_date = (datetime.now(UTC) - timedelta(days=since_days)).strftime("%d-%b-%Y")
        status, message_ids = mail.search(None, f"SINCE {since_date}")
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        logger.debug("IMAP login failed for %s: %s", username, e)
        msg = f"IMAP login failed: {e}"
        raise IMAPError(msg) from e

    if status != "OK":
        mail.logout()
        return []

    ids = message_ids[0].split() if message_ids[0] else []
    if not ids:
        mail.logout()
        return []

    messages: list[dict[str, Any]] = []
    for msg_id in ids[-max_messages:]:
        try:
            status, data = mail.fetch(msg_id, "(RFC822)")
            if status != "OK" or not data or not data[0]:
                continue
            raw: bytes = data[0][1] if isinstance(data[0][1], bytes) else b""
            if not raw:
                continue
            parsed = _parse_email(raw)
            parsed["imap_uid"] = msg_id.decode()
            messages.append(parsed)
        except (OSError, imaplib.IMAP4.error) as e:
            logger.warning("Failed to fetch IMAP message %s: %s", msg_id, e)
            continue

    mail.logout()
    return messages


def list_messages(
    folder: str = "INBOX",
    page_size: int = 20,
    page: int = 1,
    *,
    host: str = "imap.gmail.com",
    port: int = 993,
    username: str = "",
    password: str = "",
    ssl: bool = True,
) -> list[Envelope]:
    """List IMAP messages, returning a Himalaya-compatible Envelope list.

    Uses IMAP ENVELOPE fetch for header-level data only, avoiding
    full body transfer.
    """
    try:
        mail = imaplib.IMAP4_SSL(host, port) if ssl else imaplib.IMAP4(host, port)
    except (OSError, imaplib.IMAP4.error) as e:
        logger.debug("Failed to connect to %s:%s: %s", host, port, e)
        msg = f"Failed to connect to mail server: {e}"
        raise IMAPError(msg) from e

    try:
        mail.login(username, password)
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        logger.debug("IMAP login failed for %s: %s", username, e)
        msg = f"IMAP login failed: {e}"
        raise IMAPError(msg) from e

    try:
        mail.select(folder)
        since_date = (datetime.now(UTC) - timedelta(days=30)).strftime("%d-%b-%Y")
        status, message_ids = mail.search(None, f"SINCE {since_date}")
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        logger.debug("IMAP search failed: %s", e)
        msg = f"IMAP search failed: {e}"
        raise IMAPError(msg) from e

    if status != "OK":
        mail.logout()
        return []

    ids = message_ids[0].split() if message_ids[0] else []
    if not ids:
        mail.logout()
        return []

    envelopes: list[Envelope] = []
    for msg_id in ids[-(page_size * page) :]:
        try:
            status, data = mail.fetch(msg_id, "(FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)")
            if status != "OK" or not data or not data[0]:
                continue
            raw: bytes = data[0][1] if isinstance(data[0][1], bytes) else b""
            if not raw:
                continue
            parsed = _parse_envelope_response(raw.decode("utf-8", errors="replace"))
            if parsed:
                envelopes.append(
                    Envelope(
                        id=msg_id.decode(),
                        subject=parsed.get("subject", ""),
                        from_=parsed.get("from", ""),
                        to=parsed.get("to", ""),
                        date=parsed.get("date"),
                        flags=parsed.get("flags", []),
                    )
                )
        except (OSError, imaplib.IMAP4.error) as e:
            logger.warning("Failed to fetch IMAP envelope %s: %s", msg_id, e)
            continue

    mail.logout()
    start = (page - 1) * page_size
    end = start + page_size
    return envelopes[start:end]


def _parse_envelope_response(response: str) -> dict[str, Any] | None:
    """Parse IMAP ENVELOPE response into a simple dict."""
    try:
        match = re.search(r"ENVELOPE \((.*?)\) FLAGS", response, re.DOTALL)
        if not match:
            return None
        envelope_str = match.group(1)
        parts = envelope_str.split('"')
        if len(parts) >= 6:
            date_str = parts[0].strip()
            date = None
            if date_str:
                with contextlib.suppress(ValueError):
                    date = email.utils.parsedate_to_datetime(date_str)
            return {
                "subject": parts[3].strip(),
                "from": parts[5].strip(),
                "to": parts[7].strip() if len(parts) > 7 else "",
                "date": date,
                "flags": [],
            }
    except (ValueError, IndexError):
        pass
    return None


def get_message(
    message_id: str,
    *,
    host: str = "imap.gmail.com",
    port: int = 993,
    username: str = "",
    password: str = "",
    ssl: bool = True,
    folder: str = "INBOX",
) -> Message:
    """Fetch a single IMAP message by UID, returning a Himalaya-compatible Message."""
    try:
        mail = imaplib.IMAP4_SSL(host, port) if ssl else imaplib.IMAP4(host, port)
    except (OSError, imaplib.IMAP4.error) as e:
        logger.debug("Failed to connect to %s:%s: %s", host, port, e)
        msg = f"Failed to connect to mail server: {e}"
        raise IMAPError(msg) from e

    try:
        mail.login(username, password)
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        logger.debug("IMAP login failed for %s: %s", username, e)
        msg = f"IMAP login failed: {e}"
        raise IMAPError(msg) from e

    try:
        mail.select(folder)
        status, data = mail.fetch(message_id, "(RFC822)")
        if status != "OK" or not data or not data[0]:
            mail.logout()
            msg = f"Message {message_id} not found"
            raise IMAPError(msg)
        raw: bytes = data[0][1] if isinstance(data[0][1], bytes) else b""
        if not raw:
            mail.logout()
            msg = f"Message {message_id} not found"
            raise IMAPError(msg)
        parsed = _parse_email(raw)
        headers = parsed.get("headers", {})
        date_str = headers.get("Date", "")
        date = None
        if date_str:
            with contextlib.suppress(ValueError):
                date = email.utils.parsedate_to_datetime(date_str)
        mail.logout()
        return Message(
            id=message_id,
            subject=parsed.get("subject", ""),
            from_=parsed.get("from_addr", ""),
            to=headers.get("To", ""),
            date=date,
            body=parsed.get("body", ""),
            flags=[],
        )
    except IMAPError:
        raise
    except (OSError, imaplib.IMAP4.error) as e:
        mail.logout()
        msg = f"Failed to fetch message {message_id}: {e}"
        raise IMAPError(msg) from e


def match_reply_to_request(
    messages: list[dict[str, Any]],
    requests: list[dict[str, Any]],
    thread_map: dict[str, int] | None = None,
) -> list[dict[str, Any]]:
    """Match IMAP replies to removal requests by thread/subject matching.

    Parameters
    ----------
    messages:
        Inbound email dicts with at least ``subject`` and optionally ``thread_id``.
    requests:
        Removal request dicts with at least ``id`` or ``request_id`` and ``broker_id``.
    thread_map:
        Optional mapping from Message-ID string to request id. When a reply's
        ``thread_id`` is present in this map the request is matched immediately
        via ``match_method="thread"``.

    Returns
    -------
    Enriched messages with ``request_id`` and ``match_method`` keys.
    """
    matched: list[dict[str, Any]] = []
    thread_map = thread_map or {}

    # Pre-build a subject-to-request lookup index for O(1) matching
    subject_index: dict[str, Any] = {}
    for req in requests:
        req_id = req.get("id") or req.get("request_id")
        req_subject_raw = f"Data Deletion Request \u2014 {req.get('broker_id', '')}"
        subject_index[normalize_subject(req_subject_raw).lower()] = req_id

    for msg in messages:
        found = False
        msg_subject = msg.get("subject", "")
        msg_thread = msg.get("thread_id", "")

        if msg_thread and msg_thread in thread_map:
            msg["request_id"] = thread_map[msg_thread]
            msg["match_method"] = "thread"
            matched.append(msg)
            continue

        msg_subject_normalized = normalize_subject(msg_subject).lower()
        matched_req_id = subject_index.get(msg_subject_normalized)
        if matched_req_id is not None:
            msg["request_id"] = matched_req_id
            msg["match_method"] = "subject"
            found = True
            matched.append(msg)

        if not found:
            msg["request_id"] = None
            msg["match_method"] = "unmatched"
            matched.append(msg)

    return matched
