"""Repository layer for manual task queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def insert_manual_task(
    request_id: int | None,
    broker_id: str,
    broker_name: str,
    form_url: str,
    reason: str,
    instructions: str,
    screenshot_path: str,
    html_path: str,
    form_fields_json: str,
) -> int:
    conn = get_connection()
    cur = conn.execute(
        """INSERT INTO manual_tasks
           (request_id, broker_id, broker_name, form_url, reason, instructions,
            screenshot_path, html_snapshot_path, form_fields_json, status)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')""",
        (
            request_id,
            broker_id,
            broker_name,
            form_url,
            reason,
            instructions,
            screenshot_path,
            html_path,
            form_fields_json,
        ),
    )
    conn.commit()
    return cur.lastrowid  # type: ignore[return-value]


def get_manual_task(task_id: int) -> dict[str, Any] | None:
    conn = get_connection()
    row = conn.execute(
        "SELECT * FROM manual_tasks WHERE id = ?", (task_id,)
    ).fetchone()
    return dict(row) if row else None


def update_manual_task_status(
    task_id: int, status: str, completed_at: str, notes: str
) -> None:
    conn = get_connection()
    conn.execute(
        "UPDATE manual_tasks SET status = ?, completed_at = ?, notes = ? WHERE id = ?",
        (status, completed_at, notes, task_id),
    )
    conn.commit()


def list_manual_tasks(
    status: str | None = None,
    request_id: int | None = None,
) -> list[dict[str, Any]]:
    conn = get_connection()
    conditions: list[str] = []
    params: list[Any] = []
    if status:
        conditions.append("status = ?")
        params.append(status)
    if request_id is not None:
        conditions.append("request_id = ?")
        params.append(request_id)
    where = " AND ".join(conditions) if conditions else "1=1"
    rows = conn.execute(
        f"SELECT * FROM manual_tasks WHERE {where} ORDER BY created_at DESC",
        params,
    ).fetchall()
    return [dict(r) for r in rows]
