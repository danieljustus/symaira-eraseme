"""HTML status dashboard — self-contained campaign visualization."""

from __future__ import annotations

import os
from datetime import UTC, datetime
from importlib import resources
from pathlib import Path
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
    from symeraseme.core.repositories.dashboard import (
        fetch_campaigns,
        fetch_recent_events,
        fetch_requests_for_campaigns,
    )

    campaigns_rows = fetch_campaigns(campaign_id)

    campaigns: list[dict[str, Any]] = []
    all_requests: list[dict[str, Any]] = []
    recent_events: list[dict[str, Any]] = []

    if campaigns_rows:
        campaign_ids = [c["id"] for c in campaigns_rows]
        requests_rows = fetch_requests_for_campaigns(campaign_ids)

        requests_by_campaign: dict[str, list[dict[str, Any]]] = {cid: [] for cid in campaign_ids}
        for row in requests_rows:
            r = dict(row)
            cid = r["campaign_id"]
            requests_by_campaign.setdefault(cid, []).append(r)

        for c in campaigns_rows:
            camp = dict(c)
            requests = requests_by_campaign.get(camp["id"], [])
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
    events_rows = fetch_recent_events(50)

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


def _dashboard_templates_dir() -> Path:
    """Find the dashboard template directory.

    Supports both source checkouts and PyPI-installed packages by
    using importlib.resources, falling back to parent-directory
    traversal for editable installs.
    """
    env_dir = os.environ.get("SYMERASEME_RESOURCES")
    if env_dir:
        return Path(env_dir) / "templates"
    pkg_root = resources.files("symeraseme")
    candidate = Path(str(pkg_root)) / "registry" / "templates"
    if candidate.exists() and any(candidate.iterdir()):
        return candidate
    for parent in Path(str(pkg_root)).parents:
        if (parent / "registry" / "templates").exists():
            return parent / "registry" / "templates"
    msg = "Could not find dashboard templates directory (registry/templates)"
    raise FileNotFoundError(msg)


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
    loader = FileSystemLoader(str(_dashboard_templates_dir()))
    env = Environment(loader=loader, autoescape=select_autoescape(["html"]))

    template = env.get_template("dashboard.html.j2")
    html = template.render(
        data=data,
        auto_refresh_seconds=auto_refresh_seconds,
        now=datetime.now(UTC),
    )
    return html
