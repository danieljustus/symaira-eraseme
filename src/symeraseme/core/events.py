"""Append-only event store for removal request lifecycle."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import Any

from symeraseme.core.db import get_connection

EVENT_TYPES = frozenset(
    {
        "PLANNED",
        "SENT",
        "SEND_FAILED",
        "BOUNCE",
        "AUTORESPONDER",
        "ACK",
        "VERIFICATION_REQUESTED",
        "VERIFICATION_PROVIDED",
        "HUMAN_ACTION_REQUIRED",
        "CONFIRMATION_LINK_CLICKED",
        "REPLY_DRAFTED",
        "REBUTTAL_SENT",
        "REMINDER_SENT",
        "DEADLINE_REACHED",
        "DPA_COMPLAINT_DRAFTED",
        "DPA_COMPLAINT_FILED",
        "CONFIRMED",
        "REJECTED_FINAL",
        "RE_SCAN_TRIGGERED",
        "NOTE_ADDED",
    }
)

VALID_SOURCES = frozenset({"system", "inbox", "user", "scheduler"})


def create_campaign(
    campaign_id: str,
    kind: str = "initial",
    notes: str | None = None,
) -> None:
    conn = get_connection()
    conn.execute(
        "INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?, ?, ?)",
        (campaign_id, kind, notes),
    )
    conn.commit()


def list_campaigns() -> list[dict[str, Any]]:
    conn = get_connection()
    rows = conn.execute(
        "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC"
    ).fetchall()
    return [dict(r) for r in rows]


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


def append_event(
    request_id: int,
    event_type: str,
    *,
    payload: dict[str, Any] | None = None,
    source: str = "system",
    occurred_at: str | None = None,
    commit: bool = True,
) -> int:
    """Append an event to the request_events log.

    When ``commit=False`` the caller is responsible for committing — used by
    ``append_event_and_project()`` to atomically bundle event + projection in
    a single transaction.
    """
    if event_type not in EVENT_TYPES:
        msg = f"Unknown event type: {event_type}. Valid: {sorted(EVENT_TYPES)}"
        raise ValueError(msg)
    if source not in VALID_SOURCES:
        msg = f"Unknown source: {source}. Valid: {sorted(VALID_SOURCES)}"
        raise ValueError(msg)

    conn = get_connection()
    now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")
    cur = conn.execute(
        """INSERT INTO request_events
           (request_id, occurred_at, recorded_at, event_type, payload_json, source)
           VALUES (?, ?, ?, ?, ?, ?)""",
        (request_id, occurred_at or now, now, event_type, json.dumps(payload or {}), source),
    )
    if commit:
        conn.commit()
    eid: int = cur.lastrowid  # type: ignore[assignment]
    return eid


def get_events(
    request_id: int,
    *,
    after_event_id: int | None = None,
) -> list[dict[str, Any]]:
    conn = get_connection()
    if after_event_id:
        rows = conn.execute(
            """SELECT id, request_id, occurred_at, recorded_at, event_type,
                      payload_json, source
               FROM request_events
               WHERE request_id = ? AND id > ?
               ORDER BY occurred_at ASC, id ASC""",
            (request_id, after_event_id),
        ).fetchall()
    else:
        rows = conn.execute(
            """SELECT id, request_id, occurred_at, recorded_at, event_type,
                      payload_json, source
               FROM request_events
               WHERE request_id = ?
               ORDER BY occurred_at ASC, id ASC""",
            (request_id,),
        ).fetchall()

    result = []
    for r in rows:
        row = dict(r)
        row["payload_json"] = json.loads(row["payload_json"])
        result.append(row)
    return result


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
) -> list[dict[str, Any]]:
    """List removal requests with optional filters.

    WARNING: The WHERE clause MUST remain a fixed string — never use
    f-strings or string concatenation to build SQL. Filtering is handled
    via ``(? IS NULL OR col = ?)`` so the query text never changes.
    """
    conn = get_connection()
    rows = conn.execute(
        """SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                  r.jurisdiction, r.template_id,
                  s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
                  s.resolved_at, s.deadline_at, s.reminders_sent, s.escalation_level
           FROM removal_requests r
           LEFT JOIN request_state s ON s.request_id = r.id
           WHERE (? IS NULL OR r.campaign_id = ?)
             AND (? IS NULL OR s.current_status = ?)
             AND (? IS NULL OR r.broker_id = ?)
           ORDER BY r.created_at ASC""",
        (
            campaign_id or None,
            campaign_id or None,
            status or None,
            status or None,
            broker_id or None,
            broker_id or None,
        ),
    ).fetchall()
    return [dict(r) for r in rows]
