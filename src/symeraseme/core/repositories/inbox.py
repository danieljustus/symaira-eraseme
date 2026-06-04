"""Repository layer for inbox reply queries."""

from __future__ import annotations

from symeraseme.core.db import get_connection


def insert_inbox_reply(
    request_id: int | None,
    message_id: str,
    thread_id: str | None,
    from_addr: str,
    subject: str,
    snippet: str,
    classified_as: str | None,
) -> int:
    conn = get_connection()
    cur = conn.execute(
        """INSERT OR IGNORE INTO inbox_replies
           (request_id, message_id, thread_id, received_at, from_addr, subject,
            snippet, classified_as)
           VALUES (?, ?, ?, datetime('now'), ?, ?, ?, ?)""",
        (request_id, message_id, thread_id, from_addr, subject, snippet, classified_as),
    )
    conn.commit()
    return cur.lastrowid  # type: ignore[return-value]
