"""Tick engine — proactive lifecycle management for removal requests."""

from __future__ import annotations

import logging
import sqlite3
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from typing import Any

from symeraseme.core.datetime_utils import parse_iso_datetime as _parse_dt

logger = logging.getLogger(__name__)

JURISDICTION_DEADLINES: dict[str, int] = {
    "GDPR": 30,
    "CCPA": 45,
    "CPRA": 45,
    "LGPD": 30,
    "PIPEDA": 30,
}

REMINDER_DAYS = 7
DPA_ESCALATION_DAYS = 14
RE_SCAN_DAYS = 90


@dataclass
class TickAction:
    request_id: int
    broker_id: str
    campaign_id: str
    current_status: str
    action_type: str
    event_type: str
    description: str
    payload: dict[str, Any] = field(default_factory=dict)
    dry_run: bool = False


def run_tick(
    *,
    dry_run: bool = False,
    reference_date: datetime | None = None,
    batch_size: int | None = None,
) -> list[TickAction]:
    """Run one tick cycle.

    Scans all removal requests and produces actions for those
    whose next_action_at <= reference_date (or now).
    """
    actions: list[TickAction] = []
    now = reference_date or datetime.now(UTC)

    from symeraseme.core.repositories.deadlines import fetch_tick_candidates

    rows = fetch_tick_candidates(now.isoformat(), batch_size=batch_size)

    for row in rows:
        req = dict(row)
        status = req.get("current_status", "PLANNED")
        actions.extend(_tick_for_request(req, status, now, dry_run=dry_run))

    return actions


def _tick_for_request(
    req: dict[str, Any],
    status: str,
    now: datetime,
    *,
    dry_run: bool = False,
) -> list[TickAction]:
    actions: list[TickAction] = []
    rid = req["id"]

    sent_at = _parse_dt(req.get("sent_at"))
    deadline_at = _parse_dt(req.get("deadline_at"))
    resolved_at = _parse_dt(req.get("resolved_at"))
    reminders_sent = req.get("reminders_sent", 0)
    escalation_level = req.get("escalation_level", 0)
    jurisdiction = req.get("jurisdiction", "GDPR")

    deadline_days = JURISDICTION_DEADLINES.get(jurisdiction, 30)

    if status == "AWAITING_ACK":
        action = _check_reminder(rid, req, sent_at, now, reminders_sent, dry_run=dry_run)
        if action:
            actions.append(action)

    elif status == "AWAITING_RESPONSE":
        action = _check_deadline(
            rid, req, sent_at, deadline_at, deadline_days, now, dry_run=dry_run
        )
        if action:
            actions.append(action)

    elif status == "OVERDUE":
        action = _check_dpa_escalation(
            rid, req, deadline_at, now, escalation_level, dry_run=dry_run
        )
        if action:
            actions.append(action)

    elif status == "CONFIRMED":
        action = _check_rescan(rid, req, resolved_at, now, dry_run=dry_run)
        if action:
            actions.append(action)

    return actions


def _check_reminder(
    rid: int,
    req: dict[str, Any],
    sent_at: datetime | None,
    now: datetime,
    reminders_sent: int,
    *,
    dry_run: bool = False,
) -> TickAction | None:
    """Send reminder if AWAITING_ACK for more than REMINDER_DAYS."""
    if sent_at is None:
        return None

    days_elapsed = (now - sent_at).days
    if days_elapsed < REMINDER_DAYS:
        return None

    # Exponential backoff: next reminder at 2^n * REMINDER_DAYS
    next_threshold = REMINDER_DAYS * (2**reminders_sent)
    if days_elapsed < next_threshold:
        return None

    return TickAction(
        request_id=rid,
        broker_id=req.get("broker_id", ""),
        campaign_id=req.get("campaign_id", ""),
        current_status="AWAITING_ACK",
        action_type="send_reminder",
        event_type="REMINDER_SENT",
        description=f"Send reminder #{reminders_sent + 1} ({days_elapsed}d since sent)",
        payload={"days_since_sent": days_elapsed, "count": reminders_sent + 1},
        dry_run=dry_run,
    )


def _check_deadline(
    rid: int,
    req: dict[str, Any],
    sent_at: datetime | None,
    deadline_at: datetime | None,
    deadline_days: int,
    now: datetime,
    *,
    dry_run: bool = False,
) -> TickAction | None:
    """Mark request as overdue if deadline has passed."""
    if deadline_at is not None and now >= deadline_at:
        return TickAction(
            request_id=rid,
            broker_id=req.get("broker_id", ""),
            campaign_id=req.get("campaign_id", ""),
            current_status="AWAITING_RESPONSE",
            action_type="mark_overdue",
            event_type="DEADLINE_REACHED",
            description=f"Deadline reached ({deadline_days}d, passed {now - deadline_at})",
            payload={
                "deadline_days": deadline_days,
                "deadline_at": deadline_at.isoformat(),
            },
            dry_run=dry_run,
        )

    if sent_at and deadline_at is None:
        effective_deadline = sent_at + timedelta(days=deadline_days)
        if now >= effective_deadline:
            return TickAction(
                request_id=rid,
                broker_id=req.get("broker_id", ""),
                campaign_id=req.get("campaign_id", ""),
                current_status="AWAITING_RESPONSE",
                action_type="mark_overdue",
                event_type="DEADLINE_REACHED",
                description=f"Deadline reached ({deadline_days}d from sent)",
                payload={
                    "deadline_days": deadline_days,
                    "deadline_at": effective_deadline.isoformat(),
                },
                dry_run=dry_run,
            )

    return None


def _check_dpa_escalation(
    rid: int,
    req: dict[str, Any],
    deadline_at: datetime | None,
    now: datetime,
    escalation_level: int,
    *,
    dry_run: bool = False,
) -> TickAction | None:
    """Generate DPA complaint after 14 days in OVERDUE."""
    if escalation_level >= 2:
        return None

    if deadline_at is None:
        return None

    days_since_deadline = (now - deadline_at).days
    if days_since_deadline < DPA_ESCALATION_DAYS:
        return None

    return TickAction(
        request_id=rid,
        broker_id=req.get("broker_id", ""),
        campaign_id=req.get("campaign_id", ""),
        current_status="OVERDUE",
        action_type="draft_dpa_complaint",
        event_type="DPA_COMPLAINT_DRAFTED",
        description=f"DPA complaint ready ({days_since_deadline}d since deadline)",
        payload={"days_since_deadline": days_since_deadline},
        dry_run=dry_run,
    )


def _check_rescan(
    rid: int,
    req: dict[str, Any],
    resolved_at: datetime | None,
    now: datetime,
    *,
    dry_run: bool = False,
) -> TickAction | None:
    """Trigger re-scan after 90 days from confirmation."""
    if resolved_at is None:
        return None

    days_since_resolved = (now - resolved_at).days
    if days_since_resolved < RE_SCAN_DAYS:
        return None

    return TickAction(
        request_id=rid,
        broker_id=req.get("broker_id", ""),
        campaign_id=req.get("campaign_id", ""),
        current_status="CONFIRMED",
        action_type="trigger_rescan",
        event_type="RE_SCAN_TRIGGERED",
        description=f"Re-scan due ({days_since_resolved}d since resolution)",
        payload={"days_since_resolved": days_since_resolved},
        dry_run=dry_run,
    )


def apply_tick_actions(
    actions: list[TickAction],
) -> list[dict[str, Any]]:
    """Apply tick actions to the event store."""
    if not actions:
        return []

    from symeraseme.core.projection import rebuild_all_states

    results: list[dict[str, Any]] = []
    for action in actions:
        if action.dry_run:
            results.append(
                {
                    "request_id": action.request_id,
                    "action": action.action_type,
                    "event_type": action.event_type,
                    "description": action.description,
                    "executed": False,
                    "dry_run": True,
                }
            )
            continue

        try:
            from symeraseme.core.projection import append_event_and_project

            append_event_and_project(
                action.request_id,
                action.event_type,
                payload=action.payload,
                source="scheduler",
            )
            results.append(
                {
                    "request_id": action.request_id,
                    "action": action.action_type,
                    "event_type": action.event_type,
                    "description": action.description,
                    "executed": True,
                    "dry_run": False,
                }
            )
        except (sqlite3.Error, ValueError, RuntimeError) as e:
            logger.error("Failed to apply tick action %s: %s", action, e)
            results.append(
                {
                    "request_id": action.request_id,
                    "action": action.action_type,
                    "event_type": action.event_type,
                    "description": action.description,
                    "executed": False,
                    "error": str(e),
                }
            )

    # Batch-rebuild all states in a single O(1) pass
    rebuild_all_states()

    return results
