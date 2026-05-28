"""Aggregated campaign reports — per-campaign statistics and exports."""

from __future__ import annotations

import csv
import io
import json
from collections import Counter
from datetime import UTC, datetime
from typing import Any

from jinja2 import Environment, FileSystemLoader, select_autoescape

# ---------------------------------------------------------------------------
# Data collection
# ---------------------------------------------------------------------------


def get_report_data(
    campaign_id: str | None = None,
    *,
    all_campaigns: bool = False,
) -> dict[str, Any]:
    """Collect and aggregate report data from the event store.

    Args:
        campaign_id: Specific campaign to report on.
        all_campaigns: If True, include all campaigns (ignores campaign_id).

    Returns:
        Aggregated report data.
    """
    from symeraseme.core.db import get_connection

    conn = get_connection()

    # Gather campaigns
    if all_campaigns:
        campaigns_rows = conn.execute(
            "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC"
        ).fetchall()
    elif campaign_id:
        campaigns_rows = conn.execute(
            "SELECT id, created_at, kind, notes FROM campaigns WHERE id = ?",
            (campaign_id,),
        ).fetchall()
    else:
        # Default: most recent campaign
        campaigns_rows = conn.execute(
            "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC LIMIT 1"
        ).fetchall()

    if not campaigns_rows:
        return _empty_report(campaign_id or "none")

    campaigns_data: list[dict[str, Any]] = []
    all_requests: list[dict[str, Any]] = []
    all_events: list[dict[str, Any]] = []

    for c_row in campaigns_rows:
        camp = dict(c_row)

        requests = conn.execute(
            """SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                      r.jurisdiction, r.template_id,
                      s.current_status, s.sent_at, s.acknowledged_at,
                      s.resolved_at, s.deadline_at, s.reminders_sent,
                      s.escalation_level
               FROM removal_requests r
               LEFT JOIN request_state s ON s.request_id = r.id
               WHERE r.campaign_id = ?
               ORDER BY r.created_at ASC""",
            (camp["id"],),
        ).fetchall()

        reqs = [dict(r) for r in requests]
        all_requests.extend(reqs)

        request_ids = [r["id"] for r in reqs]
        if request_ids:
            ev_rows = conn.execute(
                f"""SELECT id, request_id, event_type, occurred_at, source
                    FROM request_events
                    WHERE request_id IN ({",".join("?" * len(request_ids))})
                    ORDER BY occurred_at ASC""",
                request_ids,
            ).fetchall()
            events_by_rid: dict[int, list[dict]] = {}
            for ev in ev_rows:
                evd = dict(ev)
                events_by_rid.setdefault(evd["request_id"], []).append(evd)
            for req in reqs:
                req["events"] = events_by_rid.get(req["id"], [])
                all_events.extend(req["events"])
        else:
            for req in reqs:
                req["events"] = []

        camp["requests"] = reqs
        campaigns_data.append(camp)

    # Aggregate per campaign
    campaigns_agg = [_aggregate_campaign(c) for c in campaigns_data]

    # Broker leaderboard
    broker_stats = _broker_leaderboard(all_requests)

    # Jurisdiction breakdown
    jurisdiction_stats = _jurisdiction_breakdown(all_requests)

    # Timeline
    timeline = _build_timeline(all_events)

    # Historical comparison
    comparison = _historical_comparison(campaigns_agg) if len(campaigns_agg) >= 2 else {}

    # Success metrics
    success_metrics = _success_metrics(all_requests)

    # Status breakdown for all requests
    status_counts: dict[str, int] = {}
    for r in all_requests:
        s = (r.get("current_status") or "PLANNED").upper()
        status_counts[s] = status_counts.get(s, 0) + 1

    return {
        "generated_at": datetime.now(UTC).isoformat(),
        "campaigns": campaigns_agg,
        "total_campaigns": len(campaigns_agg),
        "total_requests": len(all_requests),
        "status_breakdown": dict(sorted(status_counts.items(), key=lambda x: -x[1])),
        "broker_leaderboard": broker_stats,
        "jurisdiction_stats": jurisdiction_stats,
        "timeline": timeline,
        "historical_comparison": comparison,
        "success_metrics": success_metrics,
    }


def _empty_report(campaign_id: str) -> dict[str, Any]:
    return {
        "generated_at": datetime.now(UTC).isoformat(),
        "campaigns": [],
        "total_campaigns": 0,
        "total_requests": 0,
        "status_breakdown": {},
        "broker_leaderboard": [],
        "jurisdiction_stats": [],
        "timeline": [],
        "historical_comparison": {},
        "success_metrics": {},
        "error": f"Campaign '{campaign_id}' not found or empty",
    }


def _aggregate_campaign(camp: dict[str, Any]) -> dict[str, Any]:
    """Aggregate stats for a single campaign."""
    reqs = camp.get("requests", [])
    total = len(reqs)
    status_counts: dict[str, int] = Counter(
        (r.get("current_status") or "PLANNED").upper() for r in reqs
    )

    # Response times
    response_times: list[float] = []
    for r in reqs:
        sent = r.get("sent_at")
        resolved = r.get("resolved_at")
        if sent and resolved:
            for fmt in (
                "%Y-%m-%dT%H:%M:%S",
                "%Y-%m-%dT%H:%M:%S%z",
                "%Y-%m-%d %H:%M:%S",
            ):
                try:
                    sent_dt = datetime.strptime(str(sent).rstrip("Z"), fmt).replace(tzinfo=UTC)
                    resolved_dt = datetime.strptime(str(resolved).rstrip("Z"), fmt).replace(
                        tzinfo=UTC
                    )
                    diff = (resolved_dt - sent_dt).total_seconds() / 86400
                    response_times.append(diff)
                    break
                except ValueError:
                    continue

    avg_response_time = sum(response_times) / len(response_times) if response_times else None

    return {
        "campaign_id": camp["id"],
        "created_at": camp.get("created_at", ""),
        "kind": camp.get("kind", ""),
        "total": total,
        "status_counts": dict(status_counts),
        "planned": status_counts.get("PLANNED", 0),
        "sent": status_counts.get("SENT", 0),
        "awaiting_ack": status_counts.get("AWAITING_ACK", 0),
        "awaiting_response": status_counts.get("AWAITING_RESPONSE", 0),
        "confirmed": status_counts.get("CONFIRMED", 0),
        "rejected": status_counts.get("REJECTED_FINAL", 0),
        "overdue": status_counts.get("OVERDUE", 0),
        "confirmation_rate": (round(status_counts.get("CONFIRMED", 0) / max(total, 1) * 100, 1)),
        "rejection_rate": (round(status_counts.get("REJECTED_FINAL", 0) / max(total, 1) * 100, 1)),
        "avg_response_time_days": (
            round(avg_response_time, 1) if avg_response_time is not None else None
        ),
        "total_reminders_sent": sum(r.get("reminders_sent", 0) for r in reqs),
        "requests": reqs,
    }


def _broker_leaderboard(
    requests: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Build per-broker success statistics."""
    broker_data: dict[str, dict[str, Any]] = {}
    for r in requests:
        bid = r.get("broker_id", "unknown")
        if bid not in broker_data:
            broker_data[bid] = {
                "broker_id": bid,
                "total": 0,
                "confirmed": 0,
                "rejected": 0,
                "overdue": 0,
                "pending": 0,
                "avg_response_time_days": None,
                "response_times": [],
            }
        bd = broker_data[bid]
        bd["total"] += 1
        status = (r.get("current_status") or "PLANNED").upper()
        if status == "CONFIRMED":
            bd["confirmed"] += 1
        elif status == "REJECTED_FINAL":
            bd["rejected"] += 1
        elif status == "OVERDUE":
            bd["overdue"] += 1
        else:
            bd["pending"] += 1

        # Response time
        sent = r.get("sent_at")
        resolved = r.get("resolved_at")
        if sent and resolved:
            for fmt in (
                "%Y-%m-%dT%H:%M:%S",
                "%Y-%m-%dT%H:%M:%S%z",
                "%Y-%m-%d %H:%M:%S",
            ):
                try:
                    s = datetime.strptime(str(sent).rstrip("Z"), fmt).replace(tzinfo=UTC)
                    res = datetime.strptime(str(resolved).rstrip("Z"), fmt).replace(tzinfo=UTC)
                    bd["response_times"].append((res - s).total_seconds() / 86400)
                    break
                except ValueError:
                    continue

    result = []
    for _bid, bd in broker_data.items():
        if bd["response_times"]:
            bd["avg_response_time_days"] = round(
                sum(bd["response_times"]) / len(bd["response_times"]), 1
            )
        bd["success_rate"] = round(bd["confirmed"] / max(bd["total"], 1) * 100, 1)
        result.append(
            {
                "broker_id": bd["broker_id"],
                "total": bd["total"],
                "confirmed": bd["confirmed"],
                "rejected": bd["rejected"],
                "overdue": bd["overdue"],
                "pending": bd["pending"],
                "success_rate": bd["success_rate"],
                "avg_response_time_days": bd["avg_response_time_days"],
            }
        )

    return sorted(result, key=lambda x: -x["total"])


def _jurisdiction_breakdown(
    requests: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Break down stats by jurisdiction."""
    jdata: dict[str, dict[str, Any]] = {}
    for r in requests:
        jur = (r.get("jurisdiction") or "UNKNOWN").upper()
        if jur not in jdata:
            jdata[jur] = {
                "jurisdiction": jur,
                "total": 0,
                "confirmed": 0,
                "rejected": 0,
                "overdue": 0,
            }
        jd = jdata[jur]
        jd["total"] += 1
        status = (r.get("current_status") or "PLANNED").upper()
        if status == "CONFIRMED":
            jd["confirmed"] += 1
        elif status == "REJECTED_FINAL":
            jd["rejected"] += 1
        elif status == "OVERDUE":
            jd["overdue"] += 1

    result = []
    for _jur, jd in jdata.items():
        jd["confirmation_rate"] = round(jd["confirmed"] / max(jd["total"], 1) * 100, 1)
        result.append(jd)

    return sorted(result, key=lambda x: -x["total"])


def _build_timeline(
    events: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Build a timeline of events over time (by day)."""
    daily: dict[str, dict[str, int]] = {}
    for e in events:
        ts = e.get("occurred_at", "")
        day = ts[:10] if ts else "unknown"
        if day not in daily:
            daily[day] = Counter()
        daily[day][e.get("event_type", "UNKNOWN")] += 1

    return [
        {
            "date": day,
            "total_events": sum(counts.values()),
            "events": dict(counts),
        }
        for day, counts in sorted(daily.items())
    ]


def _historical_comparison(
    campaigns_agg: list[dict[str, Any]],
) -> dict[str, Any]:
    """Compare the most recent two campaigns."""
    if len(campaigns_agg) < 2:
        return {}
    latest = campaigns_agg[0]
    previous = campaigns_agg[1]

    return {
        "latest_campaign": latest["campaign_id"],
        "previous_campaign": previous["campaign_id"],
        "requests_change": latest["total"] - previous["total"],
        "confirmation_rate_change": round(
            (latest.get("confirmation_rate", 0) or 0) - (previous.get("confirmation_rate", 0) or 0),
            1,
        ),
        "rejection_rate_change": round(
            (latest.get("rejection_rate", 0) or 0) - (previous.get("rejection_rate", 0) or 0),
            1,
        ),
        "avg_response_time_change": (
            round(
                (latest.get("avg_response_time_days") or 0)
                - (previous.get("avg_response_time_days") or 0),
                1,
            )
            if latest.get("avg_response_time_days") is not None
            and previous.get("avg_response_time_days") is not None
            else None
        ),
    }


def _success_metrics(
    requests: list[dict[str, Any]],
) -> dict[str, Any]:
    """Calculate overall success metrics."""
    total = len(requests)
    if total == 0:
        return {}

    confirmed = sum(1 for r in requests if (r.get("current_status") or "").upper() == "CONFIRMED")
    rejected = sum(
        1 for r in requests if (r.get("current_status") or "").upper() == "REJECTED_FINAL"
    )
    overdue = sum(1 for r in requests if (r.get("current_status") or "").upper() == "OVERDUE")

    response_times: list[float] = []
    for r in requests:
        sent = r.get("sent_at")
        resolved = r.get("resolved_at")
        if sent and resolved:
            for fmt in (
                "%Y-%m-%dT%H:%M:%S",
                "%Y-%m-%dT%H:%M:%S%z",
                "%Y-%m-%d %H:%M:%S",
            ):
                try:
                    s = datetime.strptime(str(sent).rstrip("Z"), fmt).replace(tzinfo=UTC)
                    res = datetime.strptime(str(resolved).rstrip("Z"), fmt).replace(tzinfo=UTC)
                    response_times.append((res - s).total_seconds() / 86400)
                    break
                except ValueError:
                    continue

    return {
        "total_requests": total,
        "overall_confirmation_rate": round(confirmed / max(total, 1) * 100, 1),
        "overall_rejection_rate": round(rejected / max(total, 1) * 100, 1),
        "overdue_rate": round(overdue / max(total, 1) * 100, 1),
        "avg_response_time_days": (
            round(sum(response_times) / len(response_times), 1) if response_times else None
        ),
        "median_response_time_days": (_median(sorted(response_times)) if response_times else None),
    }


def _median(sorted_values: list[float]) -> float:
    n = len(sorted_values)
    if n == 0:
        return 0.0
    if n % 2 == 1:
        return sorted_values[n // 2]
    return (sorted_values[n // 2 - 1] + sorted_values[n // 2]) / 2


# ---------------------------------------------------------------------------
# Export formats
# ---------------------------------------------------------------------------


def export_json(data: dict[str, Any]) -> str:
    """Export report data as JSON string."""
    return json.dumps(data, indent=2, default=str, ensure_ascii=False)


def export_csv(data: dict[str, Any]) -> str:
    """Export campaign request data as CSV string."""
    output = io.StringIO()
    writer = csv.writer(output)

    # Header
    writer.writerow(
        [
            "campaign_id",
            "request_id",
            "broker_id",
            "jurisdiction",
            "channel",
            "status",
            "sent_at",
            "acknowledged_at",
            "resolved_at",
            "deadline_at",
            "reminders_sent",
            "escalation_level",
        ]
    )

    for camp in data.get("campaigns", []):
        for req in camp.get("requests", []):
            writer.writerow(
                [
                    camp["campaign_id"],
                    req.get("id", ""),
                    req.get("broker_id", ""),
                    req.get("jurisdiction", ""),
                    req.get("channel", ""),
                    req.get("current_status", ""),
                    req.get("sent_at", ""),
                    req.get("acknowledged_at", ""),
                    req.get("resolved_at", ""),
                    req.get("deadline_at", ""),
                    req.get("reminders_sent", 0),
                    req.get("escalation_level", 0),
                ]
            )

    return output.getvalue()


def export_html(data: dict[str, Any]) -> str:
    """Export report as an HTML page using Jinja2."""
    import pathlib

    project_root = pathlib.Path(__file__).resolve().parent.parent.parent.parent
    loader = FileSystemLoader(
        searchpath=[
            str(project_root / "registry" / "templates"),
        ]
    )
    env = Environment(loader=loader, autoescape=select_autoescape(["html"]))

    template = env.get_template("report.html.j2")
    html = template.render(
        data=data,
        now=datetime.now(UTC),
    )
    return html


# ---------------------------------------------------------------------------
# Main API
# ---------------------------------------------------------------------------


def get_campaign_status(
    campaign_id: str | None = None,
) -> dict[str, Any]:
    """Return aggregated lifecycle status across removal requests.

    Reports counts per current_status, escalation level, channel, and an
    upcoming-action horizon (deadlines, reminders due, escalations pending).
    Optionally scoped to one campaign id.
    """
    from symeraseme.core.db import get_connection, init_db

    init_db()
    conn = get_connection()
    now = datetime.now(UTC)
    now_iso = now.isoformat()
    horizon_7 = (now + timedelta(days=7)).isoformat()
    horizon_30 = (now + timedelta(days=30)).isoformat()

    where = "WHERE (? IS NULL OR r.campaign_id = ?)"
    params: tuple = (campaign_id or None, campaign_id or None)

    total_row = conn.execute(
        f"SELECT COUNT(*) AS n FROM removal_requests r {where}", params
    ).fetchone()
    total = total_row["n"] if total_row else 0

    status_counts: dict[str, int] = {}
    if total:
        rows = conn.execute(
            f"""SELECT COALESCE(s.current_status, 'PLANNED') AS status, COUNT(*) AS n
                FROM removal_requests r
                LEFT JOIN request_state s ON s.request_id = r.id
                {where}
                GROUP BY status""",
            params,
        ).fetchall()
        status_counts = {row["status"]: row["n"] for row in rows}

    channel_counts: dict[str, int] = {}
    if total:
        rows = conn.execute(
            f"SELECT r.channel, COUNT(*) AS n FROM removal_requests r {where} GROUP BY r.channel",
            params,
        ).fetchall()
        channel_counts = {row["channel"]: row["n"] for row in rows}

    escalation_counts: dict[int, int] = {0: 0, 1: 0, 2: 0}
    if total:
        rows = conn.execute(
            f"""SELECT COALESCE(s.escalation_level, 0) AS level, COUNT(*) AS n
                FROM removal_requests r
                LEFT JOIN request_state s ON s.request_id = r.id
                {where}
                GROUP BY level""",
            params,
        ).fetchall()
        for row in rows:
            escalation_counts[int(row["level"])] = row["n"]

    overdue_row = conn.execute(
        f"""SELECT COUNT(*) AS n
            FROM removal_requests r
            JOIN request_state s ON s.request_id = r.id
            {where}
              AND s.deadline_at IS NOT NULL
              AND s.deadline_at <= ?
              AND s.resolved_at IS NULL""",
        (*params, now_iso),
    ).fetchone()
    overdue = overdue_row["n"] if overdue_row else 0

    due_7_row = conn.execute(
        f"""SELECT COUNT(*) AS n
            FROM removal_requests r
            JOIN request_state s ON s.request_id = r.id
            {where}
              AND s.deadline_at IS NOT NULL
              AND s.deadline_at BETWEEN ? AND ?
              AND s.resolved_at IS NULL""",
        (*params, now_iso, horizon_7),
    ).fetchone()
    due_within_7d = due_7_row["n"] if due_7_row else 0

    due_30_row = conn.execute(
        f"""SELECT COUNT(*) AS n
            FROM removal_requests r
            JOIN request_state s ON s.request_id = r.id
            {where}
              AND s.deadline_at IS NOT NULL
              AND s.deadline_at BETWEEN ? AND ?
              AND s.resolved_at IS NULL""",
        (*params, now_iso, horizon_30),
    ).fetchone()
    due_within_30d = due_30_row["n"] if due_30_row else 0

    next_tick_row = conn.execute(
        f"""SELECT COUNT(*) AS n
            FROM removal_requests r
            JOIN request_state s ON s.request_id = r.id
            {where}
              AND s.next_action_at IS NOT NULL
              AND s.next_action_at <= ?
              AND s.resolved_at IS NULL""",
        (*params, now_iso),
    ).fetchone()
    next_tick_ready = next_tick_row["n"] if next_tick_row else 0

    resolved_row = conn.execute(
        f"""SELECT COUNT(*) AS n
            FROM removal_requests r
            JOIN request_state s ON s.request_id = r.id
            {where} AND s.resolved_at IS NOT NULL""",
        params,
    ).fetchone()
    resolved = resolved_row["n"] if resolved_row else 0

    return {
        "schema_version": 1,
        "as_of": now_iso,
        "scope": {"campaign_id": campaign_id} if campaign_id else {"campaign_id": "all"},
        "totals": {
            "requests": total,
            "resolved": resolved,
            "open": total - resolved,
        },
        "by_status": status_counts,
        "by_channel": channel_counts,
        "escalation": {
            "none": escalation_counts.get(0, 0),
            "reminder": escalation_counts.get(1, 0),
            "dpa_pending": escalation_counts.get(2, 0),
        },
        "upcoming": {
            "overdue": overdue,
            "deadline_due_within_7d": due_within_7d,
            "deadline_due_within_30d": due_within_30d,
            "tick_actions_ready": next_tick_ready,
        },
    }


def generate_report(
    data: dict[str, Any],
    format: str = "html",
) -> str | dict[str, Any]:
    """Generate a report in the requested format.

    Args:
        data: Report data from get_report_data().
        format: Output format: "html", "json", or "csv".

    Returns:
        String (HTML/CSV) or dict (JSON).
    """
    format = format.lower()
    if format == "json":
        return export_json(data)
    elif format == "csv":
        return export_csv(data)
    elif format == "html":
        return export_html(data)
    else:
        msg = f"Unsupported format: {format}. Choose html, json, or csv."
        raise ValueError(msg)
