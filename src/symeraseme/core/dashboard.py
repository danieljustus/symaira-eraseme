"""HTML status dashboard — self-contained campaign visualization."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from jinja2 import Environment, FileSystemLoader, select_autoescape


def _get_status_count(requests: list[dict[str, Any]], status: str) -> int:
    return sum(1 for r in requests if (r.get("current_status") or "").upper() == status.upper())


def get_dashboard_data(
    campaign_id: str | None = None,
) -> dict[str, Any]:
    """Collect dashboard data from the event store.

    Returns a dict with all data needed for the dashboard template.
    """
    from symeraseme.core.db import get_connection

    conn = get_connection()

    campaigns_rows = conn.execute(
        "SELECT id, created_at, kind FROM campaigns ORDER BY created_at DESC"
    ).fetchall()

    campaigns: list[dict[str, Any]] = []
    all_requests: list[dict[str, Any]] = []
    recent_events: list[dict[str, Any]] = []

    for c in campaigns_rows:
        camp = dict(c)
        if campaign_id and camp["id"] != campaign_id:
            continue

        requests_rows = conn.execute(
            """SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                      r.jurisdiction, r.template_id,
                      s.current_status, s.last_event_at, s.sent_at,
                      s.acknowledged_at, s.resolved_at, s.deadline_at,
                      s.reminders_sent, s.escalation_level
               FROM removal_requests r
               LEFT JOIN request_state s ON s.request_id = r.id
               WHERE r.campaign_id = ?
               ORDER BY r.created_at ASC""",
            (camp["id"],),
        ).fetchall()

        requests = [dict(r) for r in requests_rows]
        all_requests.extend(requests)

        camp["requests"] = requests
        camp["total"] = len(requests)
        camp["planned"] = _get_status_count(requests, "PLANNED")
        camp["sent"] = _get_status_count(requests, "SENT")
        camp["awaiting_ack"] = _get_status_count(requests, "AWAITING_ACK")
        camp["awaiting_response"] = _get_status_count(requests, "AWAITING_RESPONSE")
        camp["confirmed"] = _get_status_count(requests, "CONFIRMED")
        camp["rejected"] = _get_status_count(requests, "REJECTED_FINAL")
        camp["overdue"] = _get_status_count(requests, "OVERDUE")
        campaigns.append(camp)

    # Recent events
    events_rows = conn.execute(
        """SELECT e.id, e.request_id, e.occurred_at, e.event_type,
                  e.source, r.broker_id
           FROM request_events e
           LEFT JOIN removal_requests r ON r.id = e.request_id
           ORDER BY e.occurred_at DESC
           LIMIT 50"""
    ).fetchall()

    recent_events = [dict(e) for e in events_rows]

    # Broker status aggregation
    broker_status: dict[str, dict[str, Any]] = {}
    for r in all_requests:
        bid = r.get("broker_id", "unknown")
        if bid not in broker_status:
            broker_status[bid] = {
                "broker_id": bid,
                "total": 0,
                "confirmed": 0,
                "rejected": 0,
                "overdue": 0,
                "pending": 0,
            }
        bs = broker_status[bid]
        bs["total"] += 1
        status = (r.get("current_status") or "").upper()
        if status == "CONFIRMED":
            bs["confirmed"] += 1
        elif status == "REJECTED_FINAL":
            bs["rejected"] += 1
        elif status == "OVERDUE":
            bs["overdue"] += 1
        else:
            bs["pending"] += 1

    return {
        "campaigns": campaigns,
        "total_requests": len(all_requests),
        "planned": _get_status_count(all_requests, "PLANNED"),
        "sent": _get_status_count(all_requests, "SENT"),
        "awaiting_ack": _get_status_count(all_requests, "AWAITING_ACK"),
        "awaiting_response": _get_status_count(all_requests, "AWAITING_RESPONSE"),
        "confirmed": _get_status_count(all_requests, "CONFIRMED"),
        "rejected": _get_status_count(all_requests, "REJECTED_FINAL"),
        "overdue": _get_status_count(all_requests, "OVERDUE"),
        "broker_status": sorted(broker_status.values(), key=lambda x: x["total"], reverse=True),
        "recent_events": recent_events,
        "generated_at": datetime.now(UTC).isoformat(),
    }


def generate_dashboard(
    data: dict[str, Any],
    *,
    auto_refresh_seconds: int = 0,
) -> str:
    """Render the dashboard HTML using Jinja2.

    Args:
        data: Dashboard data dict from get_dashboard_data().
        auto_refresh_seconds: If >0, include meta refresh tag.

    Returns:
        Self-contained HTML string.
    """
    import pathlib

    # Navigate from dashboard.py -> core -> symeraseme -> src -> project root
    project_root = pathlib.Path(__file__).resolve().parent.parent.parent.parent
    loader = FileSystemLoader(
        searchpath=[
            str(project_root / "registry" / "templates"),
        ]
    )
    env = Environment(loader=loader, autoescape=select_autoescape(["html"]))

    template = env.get_template("dashboard.html.j2")
    html = template.render(
        data=data,
        auto_refresh_seconds=auto_refresh_seconds,
        now=datetime.now(UTC),
    )
    return html
