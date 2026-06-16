"""Repository layer for event queries."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import Any

from symeraseme.core.db_connection import get_connection


def append_event(
    request_id: int,
    event_type: str,
    *,
    payload: dict[str, Any] | None = None,
    source: str = "system",
    occurred_at: str | None = None,
    commit: bool = True,
) -> int:
    conn = get_connection()
    now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")
    cur = conn.execute(
        """INSERT INTO request_events
           (request_id, occurred_at, recorded_at, event_type, payload_json, source)
           VALUES (?, ?, ?, ?, ?, ?)""",
        (request_id, occurred_at or now, now, event_type, json.dumps(payload or {}), source),
    )
    if commit:
        conn.commit()
    eid: int = cur.lastrowid  # type: ignore[assignment]
    return eid


def get_events_for_requests(request_ids: list[int]) -> dict[int, list[dict[str, Any]]]:
    conn = get_connection()
    if not request_ids:
        return {}
    placeholders = ",".join("?" * len(request_ids))
    rows = conn.execute(
        f"""SELECT id, request_id, occurred_at, recorded_at, event_type,
                  payload_json, source
           FROM request_events
           WHERE request_id IN ({placeholders})
           ORDER BY occurred_at ASC, id ASC""",
        request_ids,
    ).fetchall()
    result: dict[int, list[dict[str, Any]]] = {rid: [] for rid in request_ids}
    for r in rows:
        row = dict(r)
        row["payload_json"] = json.loads(row["payload_json"])
        result[row["request_id"]].append(row)
    return result


def get_events(
    request_id: int,
    *,
    after_event_id: int | None = None,
) -> list[dict[str, Any]]:
    conn = get_connection()
    if after_event_id:
        rows = conn.execute(
            """SELECT id, request_id, occurred_at, recorded_at, event_type,
                      payload_json, source
               FROM request_events
               WHERE request_id = ? AND id > ?
               ORDER BY occurred_at ASC, id ASC""",
            (request_id, after_event_id),
        ).fetchall()
    else:
        rows = conn.execute(
            """SELECT id, request_id, occurred_at, recorded_at, event_type,
                      payload_json, source
               FROM request_events
               WHERE request_id = ?
               ORDER BY occurred_at ASC, id ASC""",
            (request_id,),
        ).fetchall()

    result = []
    for r in rows:
        row = dict(r)
        row["payload_json"] = json.loads(row["payload_json"])
        result.append(row)
    return result
