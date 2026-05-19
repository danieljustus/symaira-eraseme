"""Projection: builds request_state from the event stream."""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta
from typing import Any

from openeraseme.core.db import get_connection


def _parse_ts(value: str) -> datetime | None:
    if not value:
        return None
    for fmt in (
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%d %H:%M:%S",
    ):
        try:
            return datetime.strptime(value.rstrip("Z"), fmt).replace(tzinfo=UTC)
        except ValueError:
            continue
    return None


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


def rebuild_state(request_id: int) -> dict[str, Any]:
    conn = get_connection()
    events = conn.execute(
        """SELECT id, event_type, occurred_at, payload_json
           FROM request_events
           WHERE request_id = ?
           ORDER BY occurred_at ASC, id ASC""",
        (request_id,),
    ).fetchall()

    state: dict[str, Any] = {
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


def upsert_state(request_id: int) -> dict[str, Any]:
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
    conn.commit()
    return state


def rebuild_all_states() -> int:
    conn = get_connection()
    request_ids = conn.execute("SELECT id FROM removal_requests").fetchall()
    count = 0
    for r in request_ids:
        upsert_state(r["id"])
        count += 1
    return count
