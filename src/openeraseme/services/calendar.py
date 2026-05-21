"""Deadline / next-action calendar CLI handler."""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta
from typing import Any

from openeraseme.core.db import get_connection, init_db


def handle_calendar(
    weeks: int = 4,
    campaign_id: str | None = None,
    output_format: str = "text",
) -> str:
    """Show deadlines and scheduled actions in the next N weeks.

    Reads from ``request_state`` and groups by ISO calendar week.
    """
    init_db()
    if weeks < 1:
        weeks = 1
    conn = get_connection()
    now = datetime.now(UTC)
    horizon = now + timedelta(weeks=weeks)
    now_iso = now.isoformat()
    horizon_iso = horizon.isoformat()

    rows = conn.execute(
        """SELECT r.id AS request_id,
                  r.broker_id,
                  r.campaign_id,
                  r.jurisdiction,
                  s.current_status,
                  s.sent_at,
                  s.deadline_at,
                  s.next_action_at,
                  s.reminders_sent,
                  s.escalation_level
           FROM removal_requests r
           JOIN request_state s ON s.request_id = r.id
           WHERE (? IS NULL OR r.campaign_id = ?)
             AND s.resolved_at IS NULL
             AND (
                 (s.deadline_at IS NOT NULL AND s.deadline_at <= ?)
              OR (s.next_action_at IS NOT NULL AND s.next_action_at <= ?)
             )
           ORDER BY COALESCE(s.next_action_at, s.deadline_at) ASC""",
        (campaign_id or None, campaign_id or None, horizon_iso, horizon_iso),
    ).fetchall()

    entries: list[dict] = []
    for row in rows:
        req = dict(row)
        # Build entries for whichever marker falls within the horizon.
        deadline = req.get("deadline_at")
        next_action = req.get("next_action_at")
        marker_at = next_action or deadline
        kind = "next_action" if next_action else "deadline"
        marker_dt = _safe_parse(marker_at) if marker_at else None
        if marker_dt is None:
            continue
        days_from_now = (marker_dt - now).days
        entries.append(
            {
                "request_id": req["request_id"],
                "broker_id": req["broker_id"],
                "campaign_id": req["campaign_id"],
                "jurisdiction": req["jurisdiction"],
                "current_status": req["current_status"],
                "marker": kind,
                "marker_at": marker_at,
                "days_from_now": days_from_now,
                "overdue": days_from_now < 0,
                "deadline_at": deadline,
                "next_action_at": next_action,
                "escalation_level": req["escalation_level"],
                "reminders_sent": req["reminders_sent"],
            }
        )

    # Bucket by ISO week (YYYY-Www) for an at-a-glance view.
    buckets: dict[str, list[dict]] = {}
    for e in entries:
        marker_dt = _safe_parse(e["marker_at"])
        if marker_dt is None:
            continue
        iso = marker_dt.isocalendar()
        week_key = f"{iso.year}-W{iso.week:02d}"
        buckets.setdefault(week_key, []).append(e)

    payload: dict[str, Any] = {
        "schema_version": 1,
        "as_of": now_iso,
        "horizon_weeks": weeks,
        "horizon_until": horizon_iso,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "entries": len(entries),
            "overdue": sum(1 for e in entries if e["overdue"]),
            "weeks_with_actions": len(buckets),
        },
        "weeks": [{"week": week, "entries": items} for week, items in sorted(buckets.items())],
    }

    if output_format == "json":
        return json.dumps(payload, indent=2, default=str)

    scope = f"campaign={campaign_id}" if campaign_id else "all campaigns"
    lines = [
        f"Calendar ({scope}) — next {weeks} weeks (until {horizon_iso[:10]})",
        f"  Total upcoming entries: {len(entries)}  Overdue: {payload['totals']['overdue']}",
    ]
    if not entries:
        lines.append("")
        lines.append("Nothing scheduled in the horizon.")
        return "\n".join(lines)

    for bucket in payload["weeks"]:
        lines.append("")
        lines.append(f"Week {bucket['week']} ({len(bucket['entries'])} entries):")
        for e in bucket["entries"]:
            flag = " OVERDUE" if e["overdue"] else ""
            marker_short = (e["marker_at"] or "")[:16]
            lines.append(
                f"  #{e['request_id']:<5} {e['broker_id']:<24} "
                f"{e['current_status']:<20} "
                f"{e['marker']:<11} @ {marker_short} "
                f"({e['days_from_now']:+d}d){flag}"
            )
    return "\n".join(lines)


def _safe_parse(value: str | None) -> datetime | None:
    if not value:
        return None
    for fmt in (
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%dT%H:%M:%S.%f",
        "%Y-%m-%dT%H:%M:%S.%f%z",
        "%Y-%m-%d %H:%M:%S",
    ):
        try:
            return datetime.strptime(value.rstrip("Z"), fmt).replace(tzinfo=UTC)
        except ValueError:
            continue
    return None
