"""Repository layer for removal request queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def create_removal_request(
    *,
    broker_id: str,
    channel: str = "email",
    campaign_id: str,
    jurisdiction: str,
    template_id: str = "",
    identity_snapshot_hash: str = "",
) -> int:
    conn = get_connection()
    cur = conn.execute(
        """INSERT INTO removal_requests
           (broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash)
           VALUES (?, ?, ?, ?, ?, ?)""",
        (broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash),
    )
    conn.commit()
    rid: int = cur.lastrowid  # type: ignore[assignment]
    return rid


def get_removal_request(request_id: int) -> dict[str, Any] | None:
    conn = get_connection()
    row = conn.execute(
        "SELECT id, broker_id, channel, campaign_id, created_at, jurisdiction, "
        "template_id, identity_snapshot_hash FROM removal_requests WHERE id = ?",
        (request_id,),
    ).fetchone()
    return dict(row) if row else None


def list_removal_requests(
    *,
    campaign_id: str | None = None,
    status: str | None = None,
    broker_id: str | None = None,
    limit: int | None = None,
    offset: int | None = None,
) -> list[dict[str, Any]]:
    """List removal requests with optional filters.

    WARNING: The WHERE clause MUST remain a fixed string — never use
    f-strings or string concatenation to build SQL. Filtering is handled
    via ``(? IS NULL OR col = ?)`` so the query text never changes.

    Args:
        limit: Max rows to return (None = unlimited).
        offset: Row offset for pagination (requires limit).
    """
    conn = get_connection()
    query = """SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
              r.jurisdiction, r.template_id, r.identity_snapshot_hash,
              s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
              s.resolved_at, s.deadline_at, s.next_action_at, s.reminders_sent,
              s.escalation_level
       FROM removal_requests r
       LEFT JOIN request_state s ON s.request_id = r.id
       WHERE (? IS NULL OR r.campaign_id = ?)
         AND (? IS NULL OR s.current_status = ?)
         AND (? IS NULL OR r.broker_id = ?)
       ORDER BY r.created_at ASC"""
    params: list = [
        campaign_id or None,
        campaign_id or None,
        status or None,
        status or None,
        broker_id or None,
        broker_id or None,
    ]
    if limit is not None:
        query += " LIMIT ?"
        params.append(limit)
        if offset is not None:
            query += " OFFSET ?"
            params.append(offset)
    elif offset is not None:
        query += " LIMIT -1 OFFSET ?"
        params.append(offset)
    rows = conn.execute(query, params).fetchall()
    return [dict(r) for r in rows]
