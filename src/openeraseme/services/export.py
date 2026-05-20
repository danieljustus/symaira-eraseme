"""Audit-trail export CLI handler."""

from __future__ import annotations

import csv
import io
import json
from pathlib import Path
from typing import Any

from openeraseme.core.db import get_connection, init_db


def handle_export(
    output_file: str | None = None,
    fmt: str = "json",
    campaign_id: str | None = None,
    output_format: str = "text",
) -> str:
    """Export every removal request with its full event history.

    Lets the user keep an offline audit trail for GDPR record-keeping
    purposes. Supports JSON (default) and CSV (events flattened).

    Parameters
    ----------
    output_file : str | None
        Path to write the exported data to. When None, the serialized
        payload is returned in the result string (json) or summary text.
    fmt : {"json", "csv"}
        Export format.
    campaign_id : str | None
        Optional campaign filter.
    output_format : str
        ``text`` returns a human summary; ``json`` returns the export
        payload wrapped in a structured envelope.
    """
    init_db()
    if fmt not in ("json", "csv"):
        msg = f"Unsupported export format: {fmt}. Use 'json' or 'csv'."
        raise ValueError(msg)

    conn = get_connection()

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
    for r in request_rows:
        req = dict(r)
        events = conn.execute(
            """SELECT id, occurred_at, recorded_at, event_type, payload_json, source
               FROM request_events
               WHERE request_id = ?
               ORDER BY occurred_at ASC, id ASC""",
            (req["id"],),
        ).fetchall()
        event_dicts = []
        for e in events:
            ed = dict(e)
            try:
                ed["payload"] = json.loads(ed.pop("payload_json") or "{}")
            except json.JSONDecodeError:
                ed["payload"] = {}
            event_dicts.append(ed)
            flat_events.append({"request_id": req["id"], **ed})
        req["events"] = event_dicts
        requests.append(req)

    if fmt == "json":
        payload = {
            "schema_version": 1,
            "scope": {"campaign_id": campaign_id or "all"},
            "totals": {
                "requests": len(requests),
                "events": sum(len(r["events"]) for r in requests),
            },
            "requests": requests,
        }
        serialized = json.dumps(payload, indent=2, default=str)
    else:
        # CSV: flatten the event log; one row per event.
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
            # Still write request rows (without events) so the export isn't empty.
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
        serialized = buf.getvalue()

    if output_file:
        path = Path(output_file).expanduser().resolve()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(serialized, encoding="utf-8")

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
        # No output file: embed the serialized payload too.
        summary["payload"] = serialized
        return json.dumps(summary, indent=2, default=str)

    # text
    if output_file:
        return (
            f"Exported {summary['totals']['requests']} request(s) "
            f"and {summary['totals']['events']} event(s) "
            f"to {summary['output_file']} ({fmt})."
        )
    return serialized
