"""Audit-trail export CLI handler."""

from __future__ import annotations

import csv
import io
import json
import sqlite3
from pathlib import Path
from typing import Any

from symeraseme.core.db import get_connection, init_db


def _collect_export_data(
    conn: sqlite3.Connection,
    campaign_id: str | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Query removal requests and events, building the canonical export structures.

    Returns (requests, flat_events) where each request has its events attached.
    """
    request_rows = conn.execute(
        """SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                  r.jurisdiction, r.template_id, r.identity_snapshot_hash,
                  s.current_status, s.sent_at, s.acknowledged_at, s.resolved_at,
                  s.deadline_at, s.next_action_at, s.reminders_sent,
                  s.escalation_level, s.last_event_at
           FROM removal_requests r
           LEFT JOIN request_state s ON s.request_id = r.id
           WHERE (? IS NULL OR r.campaign_id = ?)
           ORDER BY r.id ASC""",
        (campaign_id or None, campaign_id or None),
    ).fetchall()

    requests: list[dict[str, Any]] = []
    flat_events: list[dict[str, Any]] = []

    req_ids = [r["id"] for r in request_rows]
    if req_ids:
        ev_rows = conn.execute(
            f"""SELECT id, request_id, occurred_at, recorded_at, event_type,
                       payload_json, source
                FROM request_events
                WHERE request_id IN ({",".join("?" * len(req_ids))})
                ORDER BY occurred_at ASC, id ASC""",
            req_ids,
        ).fetchall()
        events_by_rid: dict[int, list[dict]] = {}
        for ev in ev_rows:
            evd = dict(ev)
            try:
                evd["payload"] = json.loads(evd.pop("payload_json") or "{}")
            except json.JSONDecodeError:
                evd["payload"] = {}
            events_by_rid.setdefault(evd["request_id"], []).append(evd)
            flat_events.append({"request_id": evd["request_id"], **evd})
    else:
        events_by_rid = {}

    for r in request_rows:
        req = dict(r)
        req["events"] = events_by_rid.get(req["id"], [])
        requests.append(req)

    return requests, flat_events


def _format_export_json(
    requests: list[dict[str, Any]],
    campaign_id: str | None,
) -> str:
    """Serialize the export payload as JSON."""
    payload = {
        "schema_version": 1,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "requests": len(requests),
            "events": sum(len(r["events"]) for r in requests),
        },
        "requests": requests,
    }
    return json.dumps(payload, indent=2, default=str)


def _format_export_csv(
    requests: list[dict[str, Any]],
    flat_events: list[dict[str, Any]],
) -> str:
    """Flatten the event log into CSV rows (one row per event)."""
    buf = io.StringIO()
    writer = csv.writer(buf)
    writer.writerow(
        [
            "request_id",
            "broker_id",
            "campaign_id",
            "jurisdiction",
            "current_status",
            "event_id",
            "occurred_at",
            "event_type",
            "source",
            "payload_json",
        ]
    )
    req_by_id = {r["id"]: r for r in requests}
    if not flat_events:
        for req in requests:
            writer.writerow(
                [
                    req["id"],
                    req["broker_id"],
                    req["campaign_id"],
                    req["jurisdiction"],
                    req.get("current_status", ""),
                    "",
                    "",
                    "",
                    "",
                    "",
                ]
            )
    else:
        for e in flat_events:
            req = req_by_id.get(e["request_id"], {})
            writer.writerow(
                [
                    e["request_id"],
                    req.get("broker_id", ""),
                    req.get("campaign_id", ""),
                    req.get("jurisdiction", ""),
                    req.get("current_status", ""),
                    e["id"],
                    e["occurred_at"],
                    e["event_type"],
                    e["source"],
                    json.dumps(e.get("payload", {}), default=str),
                ]
            )
    return buf.getvalue()


def _write_export_file(output_file: str, serialized: str) -> None:
    """Write the serialized export payload to *output_file*."""
    path = Path(output_file).expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(serialized, encoding="utf-8")


def handle_export(
    output_file: str | None = None,
    fmt: str = "json",
    campaign_id: str | None = None,
    output_format: str = "text",
) -> str:
    """Export every removal request with its full event history."""
    init_db()
    if fmt not in ("json", "csv"):
        msg = f"Unsupported export format: {fmt}. Use 'json' or 'csv'."
        raise ValueError(msg)

    conn = get_connection()
    requests, flat_events = _collect_export_data(conn, campaign_id)

    if fmt == "json":
        serialized = _format_export_json(requests, campaign_id)
    else:
        serialized = _format_export_csv(requests, flat_events)

    if output_file:
        _write_export_file(output_file, serialized)

    summary: dict[str, Any] = {
        "schema_version": 1,
        "format": fmt,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "requests": len(requests),
            "events": sum(len(r.get("events", [])) for r in requests),
        },
        "output_file": str(Path(output_file).expanduser().resolve()) if output_file else None,
    }

    if output_format == "json":
        if output_file:
            return json.dumps(summary, indent=2, default=str)
        summary["payload"] = serialized
        return json.dumps(summary, indent=2, default=str)

    if output_file:
        return (
            f"Exported {summary['totals']['requests']} request(s) "
            f"and {summary['totals']['events']} event(s) "
            f"to {summary['output_file']} ({fmt})."
        )
    return serialized
