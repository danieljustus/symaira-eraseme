"""Repository layer for inbox reply and draft queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def list_replies(
    where_clause: str,
    params: list[str],
) -> list[dict[str, Any]]:
    conn = get_connection()
    rows = conn.execute(
        f"""SELECT r.id, r.request_id, r.message_id, r.thread_id,
                   r.received_at, r.from_addr, r.subject, r.snippet,
                   r.classified_as, r.classifier_confidence, r.llm_summary,
                   d.id AS draft_id, d.subject AS draft_subject,
                   d.created_at AS draft_created_at, d.sent_at AS draft_sent_at,
                   d.account
            FROM inbox_replies r
            LEFT JOIN reply_drafts d ON d.reply_id = r.id
                AND d.id = (
                    SELECT d2.id FROM reply_drafts d2
                    WHERE d2.reply_id = r.id
                    ORDER BY d2.created_at DESC LIMIT 1
                )
            WHERE {where_clause}
            ORDER BY r.received_at DESC""",
        params,
    ).fetchall()
    return [dict(row) for row in rows]


def get_reply(reply_id: int) -> dict[str, Any] | None:
    conn = get_connection()
    row = conn.execute(
        """SELECT r.id, r.request_id, r.message_id, r.thread_id,
                  r.received_at, r.from_addr, r.subject, r.snippet,
                  r.classified_as, r.classifier_confidence, r.llm_summary
           FROM inbox_replies r
           WHERE r.id = ?""",
        (reply_id,),
    ).fetchone()
    if row is None:
        return None
    result = dict(row)
    draft = conn.execute(
        """SELECT id, draft_body, subject, created_at, sent_at, account
           FROM reply_drafts
           WHERE reply_id = ?
           ORDER BY created_at DESC LIMIT 1""",
        (reply_id,),
    ).fetchone()
    if draft:
        d = dict(draft)
        result["draft_id"] = d["id"]
        result["draft_body"] = d["draft_body"]
        result["draft_subject"] = d["subject"]
        result["draft_created_at"] = d["created_at"]
        result["draft_sent_at"] = d["sent_at"]
        result["draft_account"] = d["account"]
    return result


def get_existing_draft_id(reply_id: int) -> int | None:
    conn = get_connection()
    row = conn.execute(
        "SELECT id FROM reply_drafts WHERE reply_id = ? AND sent_at IS NULL",
        (reply_id,),
    ).fetchone()
    return row["id"] if row else None


def get_draft_detail(draft_id: int) -> dict[str, Any] | None:
    conn = get_connection()
    row = conn.execute(
        "SELECT id, draft_body, subject FROM reply_drafts WHERE id = ?",
        (draft_id,),
    ).fetchone()
    return dict(row) if row else None


def insert_reply_draft(
    reply_id: int,
    request_id: int,
    draft_body: str,
    draft_subject: str,
    account: str | None,
) -> int:
    conn = get_connection()
    cur = conn.execute(
        """INSERT INTO reply_drafts
           (reply_id, request_id, draft_body, subject, account)
           VALUES (?, ?, ?, ?, ?)""",
        (reply_id, request_id, draft_body, draft_subject, account),
    )
    conn.commit()
    return cur.lastrowid  # type: ignore[return-value]


def get_latest_draft(reply_id: int) -> dict[str, Any] | None:
    conn = get_connection()
    row = conn.execute(
        """SELECT id, draft_body, subject, sent_at
           FROM reply_drafts
           WHERE reply_id = ?
           ORDER BY created_at DESC LIMIT 1""",
        (reply_id,),
    ).fetchone()
    return dict(row) if row else None


def mark_draft_sent(draft_id: int, account: str | None) -> None:
    conn = get_connection()
    conn.execute(
        "UPDATE reply_drafts SET sent_at = datetime('now'), account = ? WHERE id = ?",
        (account, draft_id),
    )
    conn.commit()
