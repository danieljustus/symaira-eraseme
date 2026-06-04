"""Repository layer for campaign queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def create_campaign(
    campaign_id: str,
    kind: str = "initial",
    notes: str | None = None,
) -> bool:
    conn = get_connection()
    exists = conn.execute("SELECT 1 FROM campaigns WHERE id = ?", (campaign_id,)).fetchone()
    if exists:
        return False
    conn.execute(
        "INSERT INTO campaigns (id, kind, notes) VALUES (?, ?, ?)",
        (campaign_id, kind, notes),
    )
    conn.commit()
    return True


def list_campaigns() -> list[dict[str, Any]]:
    conn = get_connection()
    rows = conn.execute(
        "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC"
    ).fetchall()
    return [dict(r) for r in rows]
