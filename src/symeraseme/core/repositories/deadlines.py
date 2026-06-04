"""Repository layer for deadline/tick engine queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def fetch_tick_candidates(
    now_iso: str,
    batch_size: int | None = None,
) -> list[dict[str, Any]]:
    conn = get_connection()
    query = """SELECT r.id, r.broker_id, r.campaign_id, r.jurisdiction,
                  s.current_status, s.sent_at, s.deadline_at, s.next_action_at,
                  s.acknowledged_at, s.resolved_at, s.reminders_sent,
                  s.escalation_level
           FROM removal_requests r
           JOIN request_state s ON s.request_id = r.id
           WHERE s.next_action_at IS NULL
              OR s.next_action_at <= ?
           ORDER BY s.next_action_at ASC"""
    params: list[Any] = [now_iso]
    if batch_size:
        query += " LIMIT ?"
        params.append(batch_size)
    rows = conn.execute(query, params).fetchall()
    return [dict(r) for r in rows]
