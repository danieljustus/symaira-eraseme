"""Projection: builds request_state from the event stream."""

from __future__ import annotations

import json
import logging
from datetime import timedelta
from typing import Any

from symeraseme.core.datetime_utils import parse_iso_datetime as _parse_ts
from symeraseme.core.db import get_connection
from symeraseme.core.events import append_event

logger = logging.getLogger(__name__)



def _next_status(event_type: str) -> str | None:
    mapping: dict[str, str | None] = {
        "PLANNED": "PLANNED",
        "SENT": "AWAITING_ACK",
        "SEND_FAILED": "SEND_FAILED",
        "BOUNCE": "BOUNCE",
        "ACK": "ACK",
        "AUTORESPONDER": "AWAITING_ACK",
        "VERIFICATION_REQUESTED": "AWAITING_USER_ACTION",
        "VERIFICATION_PROVIDED": "AWAITING_RESPONSE",
        "HUMAN_ACTION_REQUIRED": "AWAITING_USER_ACTION",
        "CONFIRMED": "CONFIRMED",
        "REJECTED_FINAL": "REJECTED_FINAL",
        "CONFIRMATION_LINK_CLICKED": "CONFIRMED",
        "REBUTTAL_SENT": "AWAITING_RESPONSE",
        "REMINDER_SENT": "AWAITING_ACK",
        "DEADLINE_REACHED": "OVERDUE",
        "DPA_COMPLAINT_DRAFTED": "ESCALATED",
        "DPA_COMPLAINT_FILED": "DPA_FILED",
        "RE_SCAN_TRIGGERED": "RE_SCAN_DUE",
        "NOTE_ADDED": None,
    }
    return mapping.get(event_type)


def _new_blank_state(request_id: int) -> dict[str, Any]:
    """Return a default/blank state for a request before any events."""
    return {
        "request_id": request_id,
        "current_status": "PLANNED",
        "last_event_id": 0,
        "last_event_at": None,
        "sent_at": None,
        "acknowledged_at": None,
        "resolved_at": None,
        "deadline_at": None,
        "next_action_at": None,
        "reminders_sent": 0,
        "escalation_level": 0,
    }


def _accumulate_state(
    request_id: int,
    events: list[dict[str, Any]],
) -> dict[str, Any]:
    """Build state dict from an ordered list of event dicts."""
    state = _new_blank_state(request_id)

    for event in events:
        event_id = event["id"]
        event_type = event["event_type"]
        occurred = _parse_ts(event["occurred_at"])
        payload = json.loads(event["payload_json"]) if event["payload_json"] else {}

        new_status = _next_status(event_type)
        if new_status is not None:
            state["current_status"] = new_status

        state["last_event_id"] = event_id
        state["last_event_at"] = occurred.isoformat() if occurred else None

        if event_type == "SENT":
            state["sent_at"] = occurred.isoformat() if occurred else None
            deadline_days = payload.get("expected_response_days", 30)
            if occurred:
                state["deadline_at"] = (occurred + timedelta(days=deadline_days)).isoformat()

        elif event_type == "ACK":
            state["acknowledged_at"] = occurred.isoformat() if occurred else None

        elif event_type in ("CONFIRMED", "REJECTED_FINAL"):
            state["resolved_at"] = occurred.isoformat() if occurred else None

        elif event_type == "REMINDER_SENT":
            state["reminders_sent"] = int(payload.get("count", 0)) or 1

        elif event_type == "DEADLINE_REACHED":
            state["escalation_level"] = 1

        elif event_type == "DPA_COMPLAINT_DRAFTED":
            state["escalation_level"] = 2

    return state


def rebuild_state(request_id: int) -> dict[str, Any]:
    conn = get_connection()
    events = conn.execute(
        """SELECT id, event_type, occurred_at, payload_json
           FROM request_events
           WHERE request_id = ?
           ORDER BY occurred_at ASC, id ASC""",
        (request_id,),
    ).fetchall()
    return _accumulate_state(request_id, events)


def upsert_state(request_id: int, *, commit: bool = True) -> dict[str, Any]:
    """Recompute the projection for one request and write it.

    When ``commit=False`` the caller is responsible for committing — used by
    ``append_event_and_project()`` to bundle event + projection atomically.
    """
    state = rebuild_state(request_id)
    conn = get_connection()
    conn.execute(
        """INSERT OR REPLACE INTO request_state
           (request_id, current_status, last_event_id, last_event_at,
            sent_at, acknowledged_at, resolved_at, deadline_at,
            next_action_at, reminders_sent, escalation_level)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            state["request_id"],
            state["current_status"],
            state["last_event_id"],
            state["last_event_at"],
            state["sent_at"],
            state["acknowledged_at"],
            state["resolved_at"],
            state["deadline_at"],
            state["next_action_at"],
            state["reminders_sent"],
            state["escalation_level"],
        ),
    )
    if commit:
        conn.commit()
    return state


def append_event_and_project(
    request_id: int,
    event_type: str,
    *,
    payload: dict[str, Any] | None = None,
    source: str = "system",
    occurred_at: str | None = None,
) -> tuple[int, dict[str, Any]]:
    """Append an event and update its projection in one atomic transaction.

    Both the ``request_events`` INSERT and the ``request_state`` upsert happen
    inside a single SQLite transaction. If the projection step raises, the
    event INSERT is rolled back as well — so the event log and the projection
    can never diverge mid-write.

    Returns
    -------
    tuple[int, dict[str, Any]]
        The newly created event id and the resulting projection state.
    """
    logger.debug("Projecting event %s for request %s", event_type, request_id)
    conn = get_connection()
    try:
        eid = append_event(
            request_id,
            event_type,
            payload=payload,
            source=source,
            occurred_at=occurred_at,
            commit=False,
        )
        state = upsert_state(request_id, commit=False)
        conn.commit()
    except Exception:
        # Narrowing is unsafe here: this is a transaction boundary that must
        # rollback on ANY error (including unexpected ones like
        # sqlite3.OperationalError or KeyboardInterrupt-during-I/O).
        # The original exception is re-raised to the caller unchanged.
        conn.rollback()
        raise
    return eid, state


def rebuild_all_states(chunk_size: int = 100) -> int:
    """Rebuild request_state for every dirty request, processing in chunks.

    When a campaign contains 1,000+ brokers, loading every event into a
    single in-memory dict can exhaust RAM.  Chunking keeps memory bounded
    regardless of campaign size while preserving event order and state
    consistency per request.
    """
    conn = get_connection()

    dirty_rows = conn.execute(
        """SELECT DISTINCT r.id AS request_id
           FROM removal_requests r
           JOIN request_events e ON e.request_id = r.id
           LEFT JOIN request_state s ON s.request_id = r.id
           WHERE s.last_event_id IS NULL OR e.id > s.last_event_id""",
    ).fetchall()
    dirty_request_ids = [row["request_id"] for row in dirty_rows]

    if not dirty_request_ids:
        return 0

    total_states = 0
    for start in range(0, len(dirty_request_ids), chunk_size):
        chunk = dirty_request_ids[start : start + chunk_size]
        placeholders = ",".join("?" * len(chunk))
        rows = conn.execute(
            f"""SELECT r.id AS request_id,
                      e.id AS id,
                      e.event_type,
                      e.occurred_at,
                      e.payload_json
               FROM removal_requests r
               JOIN request_events e ON e.request_id = r.id
               WHERE r.id IN ({placeholders})
               ORDER BY r.id, e.occurred_at ASC, e.id ASC""",
            chunk,
        ).fetchall()

        buckets: dict[int, list[dict[str, Any]]] = {}
        for row in rows:
            rid = row["request_id"]
            if rid not in buckets:
                buckets[rid] = []
            buckets[rid].append(row)

        states = [_accumulate_state(rid, events) for rid, events in buckets.items()]

        conn.executemany(
            """INSERT OR REPLACE INTO request_state
               (request_id, current_status, last_event_id, last_event_at,
                sent_at, acknowledged_at, resolved_at, deadline_at,
                next_action_at, reminders_sent, escalation_level)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            [
                (
                    s["request_id"],
                    s["current_status"],
                    s["last_event_id"],
                    s["last_event_at"],
                    s["sent_at"],
                    s["acknowledged_at"],
                    s["resolved_at"],
                    s["deadline_at"],
                    s["next_action_at"],
                    s["reminders_sent"],
                    s["escalation_level"],
                )
                for s in states
            ],
        )
        conn.commit()
        total_states += len(states)

    return total_states
