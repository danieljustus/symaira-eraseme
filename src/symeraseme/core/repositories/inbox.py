"""Repository layer for inbox reply queries."""

from __future__ import annotations

from symeraseme.core.db_connection import get_connection


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


def get_imap_hwm(
    host: str,
    folder: str,
) -> tuple[int | None, int | None]:
    """Return (uid_validity, last_uid) for the given (host, folder), or (None, None).

    ``uid_validity`` is the IMAP UIDVALIDITY value for the folder.
    ``last_uid`` is the highest UID successfully processed.
    """
    conn = get_connection()
    row = conn.execute(
        "SELECT uid_validity, last_uid FROM imap_state WHERE host = ? AND folder = ?",
        (host, folder),
    ).fetchone()
    if row is None:
        return None, None
    return int(row[0]), int(row[1])


def set_imap_hwm(
    host: str,
    folder: str,
    uid_validity: int,
    last_uid: int,
) -> None:
    """Persist the high-water mark for (host, folder).

    ``uid_validity`` resets ``last_uid`` when the server's UIDVALIDITY changes,
    so stale UIDs from a previous mailbox session are never used.
    """
    conn = get_connection()
    conn.execute(
        """INSERT INTO imap_state (host, folder, uid_validity, last_uid, updated_at)
           VALUES (?, ?, ?, ?, datetime('now'))
           ON CONFLICT(host, folder) DO UPDATE SET
               uid_validity = excluded.uid_validity,
               last_uid = excluded.last_uid,
               updated_at = excluded.updated_at""",
        (host, folder, uid_validity, last_uid),
    )
    conn.commit()
