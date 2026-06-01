"""Report data collection and aggregation from the event store."""

from __future__ import annotations

from collections import Counter
from datetime import UTC, datetime, timedelta
from typing import Any

from symeraseme.core.datetime_utils import parse_date_pair


def get_report_data(
    campaign_id: str | None = None,
    *,
    all_campaigns: bool = False,
) -> dict[str, Any]:
    """Collect and aggregate report data from the event store."""
    from symeraseme.core.db import get_connection

    conn = get_connection()

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

    campaigns_agg = [_aggregate_campaign(c) for c in campaigns_data]
    broker_stats = _broker_leaderboard(all_requests)
    jurisdiction_stats = _jurisdiction_breakdown(all_requests)
    timeline = _build_timeline(all_events)
    comparison = _historical_comparison(campaigns_agg) if len(campaigns_agg) >= 2 else {}
    success_metrics = _success_metrics(all_requests)

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
    reqs = camp.get("requests", [])
    total = len(reqs)
    status_counts: dict[str, int] = Counter(
        (r.get("current_status") or "PLANNED").upper() for r in reqs
    )

    response_times: list[float] = []
    for r in reqs:
        pair = parse_date_pair(r.get("sent_at"), r.get("resolved_at"))
        if pair:
            response_times.append((pair[1] - pair[0]).total_seconds() / 86400)

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

        pair = parse_date_pair(r.get("sent_at"), r.get("resolved_at"))
        if pair:
            bd["response_times"].append((pair[1] - pair[0]).total_seconds() / 86400)

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
        pair = parse_date_pair(r.get("sent_at"), r.get("resolved_at"))
        if pair:
            response_times.append((pair[1] - pair[0]).total_seconds() / 86400)

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


def _count_where(
    conn,
    where: str,
    params: tuple,
    *,
    join_clause: str = "",
    additional_where: str = "",
    additional_params: tuple = (),
) -> int:
    row = conn.execute(
        f"SELECT COUNT(*) AS n FROM removal_requests r {join_clause} {where} {additional_where}",
        (*params, *additional_params),
    ).fetchone()
    return row["n"] if row else 0


def get_campaign_status(
    campaign_id: str | None = None,
) -> dict[str, Any]:
    """Return aggregated lifecycle status across removal requests."""
    from symeraseme.core.db import get_connection, init_db

    init_db()
    conn = get_connection()
    now = datetime.now(UTC)
    now_iso = now.isoformat()
    horizon_7 = (now + timedelta(days=7)).isoformat()
    horizon_30 = (now + timedelta(days=30)).isoformat()

    where = "WHERE (? IS NULL OR r.campaign_id = ?)"
    params: tuple = (campaign_id or None, campaign_id or None)

    total = _count_where(conn, where, params)

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

    overdue = _count_where(
        conn, where, params,
        join_clause="JOIN request_state s ON s.request_id = r.id",
        additional_where="AND s.deadline_at IS NOT NULL AND s.deadline_at <= ? AND s.resolved_at IS NULL",
        additional_params=(now_iso,),
    )

    due_within_7d = _count_where(
        conn, where, params,
        join_clause="JOIN request_state s ON s.request_id = r.id",
        additional_where="AND s.deadline_at IS NOT NULL AND s.deadline_at BETWEEN ? AND ? AND s.resolved_at IS NULL",
        additional_params=(now_iso, horizon_7),
    )

    due_within_30d = _count_where(
        conn, where, params,
        join_clause="JOIN request_state s ON s.request_id = r.id",
        additional_where="AND s.deadline_at IS NOT NULL AND s.deadline_at BETWEEN ? AND ? AND s.resolved_at IS NULL",
        additional_params=(now_iso, horizon_30),
    )

    next_tick_ready = _count_where(
        conn, where, params,
        join_clause="JOIN request_state s ON s.request_id = r.id",
        additional_where="AND s.next_action_at IS NOT NULL AND s.next_action_at <= ? AND s.resolved_at IS NULL",
        additional_params=(now_iso,),
    )

    resolved = _count_where(
        conn, where, params,
        join_clause="JOIN request_state s ON s.request_id = r.id",
        additional_where="AND s.resolved_at IS NOT NULL",
    )

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
