"""Repository layer for campaign queries."""

from __future__ import annotations

from typing import Any

from symeraseme.core.db import get_connection


def create_campaign(
    campaign_id: str,
    kind: str = "initial",
    notes: str | None = None,
) -> None:
    conn = get_connection()
    conn.execute(
        "INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?, ?, ?)",
        (campaign_id, kind, notes),
    )
    conn.commit()


def list_campaigns() -> list[dict[str, Any]]:
    conn = get_connection()
    rows = conn.execute(
        "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC"
    ).fetchall()
    return [dict(r) for r in rows]
