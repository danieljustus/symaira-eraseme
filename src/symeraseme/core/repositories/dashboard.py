"""Repository layer for dashboard queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db_connection import get_connection


def fetch_campaigns(campaign_id: str | None = None) -> list[dict[str, Any]]:
    conn = get_connection()
    if campaign_id:
        rows = conn.execute(
            "SELECT id, created_at, kind FROM campaigns WHERE id = ? ORDER BY created_at DESC",
            (campaign_id,),
        ).fetchall()
    else:
        rows = conn.execute(
            "SELECT id, created_at, kind FROM campaigns ORDER BY created_at DESC"
        ).fetchall()
    return [dict(r) for r in rows]


def fetch_requests_for_campaigns(
    campaign_ids: list[str],
) -> list[dict[str, Any]]:
    conn = get_connection()
    placeholders = ",".join("?" for _ in campaign_ids)
    rows = conn.execute(
        f"""SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                   r.jurisdiction, r.template_id,
                   s.current_status, s.last_event_at, s.sent_at,
                   s.acknowledged_at, s.resolved_at, s.deadline_at,
                   s.reminders_sent, s.escalation_level
            FROM removal_requests r
            LEFT JOIN request_state s ON s.request_id = r.id
            WHERE r.campaign_id IN ({placeholders})
            ORDER BY r.created_at ASC""",
        campaign_ids,
    ).fetchall()
    return [dict(r) for r in rows]


def fetch_recent_events(limit: int = 50) -> list[dict[str, Any]]:
    conn = get_connection()
    rows = conn.execute(
        """SELECT e.id, e.request_id, e.occurred_at, e.event_type,
                  e.source, r.broker_id
           FROM request_events e
           LEFT JOIN removal_requests r ON r.id = e.request_id
           ORDER BY e.occurred_at DESC
           LIMIT ?""",
        (limit,),
    ).fetchall()
    return [dict(e) for e in rows]
