"""Inbox reply storage and event recording."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def submit_inbox_reply(
    message_id: str,
    *,
    request_id: int | None = None,
    thread_id: str | None = None,
    from_addr: str = "",
    subject: str = "",
    snippet: str = "",
    classified_as: str | None = None,
) -> dict[str, Any]:
    """Store an inbox reply and append corresponding event."""
    conn = get_connection()
    cur = conn.execute(
        """INSERT OR IGNORE INTO inbox_replies
           (request_id, message_id, thread_id, received_at, from_addr, subject,
            snippet, classified_as)
           VALUES (?, ?, ?, datetime('now'), ?, ?, ?, ?)""",
        (request_id, message_id, thread_id, from_addr, subject, snippet, classified_as),
    )
    conn.commit()
    reply_id = cur.lastrowid

    return {"reply_id": reply_id, "request_id": request_id, "classified_as": classified_as}
