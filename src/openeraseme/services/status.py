"""Aggregated campaign status CLI handler."""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta

from openeraseme.core.db import get_connection, init_db


def handle_status(
    campaign_id: str | None = None,
    output_format: str = "text",
) -> str:
    """Return an aggregated lifecycle view across all removal requests.

    Reports counts per current_status, escalation level, channel, and an
    upcoming-action horizon (deadlines, reminders due, escalations pending).
    Optionally scoped to one campaign id.
    """
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

    summary = {
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

    if output_format == "json":
        return json.dumps(summary, indent=2, default=str)

    scope = f"campaign={campaign_id}" if campaign_id else "all campaigns"
    lines = [
        f"Status ({scope}) as of {now_iso}",
        f"  Total: {total}   Resolved: {resolved}   Open: {total - resolved}",
    ]
    if status_counts:
        lines.append("  By status:")
        for status, count in sorted(status_counts.items(), key=lambda kv: -kv[1]):
            lines.append(f"    {status:<22} {count}")
    if channel_counts:
        lines.append("  By channel:")
        for channel, count in sorted(channel_counts.items()):
            lines.append(f"    {channel:<22} {count}")
    lines.append("  Escalation:")
    lines.append(f"    none           {escalation_counts.get(0, 0)}")
    lines.append(f"    reminder sent  {escalation_counts.get(1, 0)}")
    lines.append(f"    dpa pending    {escalation_counts.get(2, 0)}")
    lines.append("  Upcoming:")
    lines.append(f"    overdue              {overdue}")
    lines.append(f"    deadline within 7d   {due_within_7d}")
    lines.append(f"    deadline within 30d  {due_within_30d}")
    lines.append(f"    tick actions ready   {next_tick_ready}")
    return "\n".join(lines)
