"""Event-store contract conformance tests (issue #709).

The golden database (tests/fixtures/event-store/golden-campaign.db) is a
committed SQLite file built through the real repository layer. This test
copies it to a temp location and rebuilds the request_state projection from
the event log alone, then compares the result against the committed
golden-projection.json. The Go port will replay the identical file through
its own projection and must produce the identical JSON.
"""

from __future__ import annotations

import json
import os
import shutil

from symeraseme.core.db_connection import close_connection, init_db
from symeraseme.core.projection import rebuild_state

FIXTURE_DIR = "tests/fixtures/event-store"
DB_FIXTURE = f"{FIXTURE_DIR}/golden-campaign.db"
PROJECTION_FIXTURE = f"{FIXTURE_DIR}/golden-projection.json"


def _load_db_in(tmp_path) -> None:
    """Install the golden DB as the active database file."""
    shutil.copy2(DB_FIXTURE, tmp_path / "symeraseme.db")
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    close_connection()
    init_db()


def test_golden_db_rebuilds_to_committed_projection(tmp_path):
    _load_db_in(tmp_path)
    with open(PROJECTION_FIXTURE) as f:
        expected = json.load(f)

    rebuilt = {rid: rebuild_state(int(rid)) for rid in expected}
    assert rebuilt == expected, (
        "Projection rebuilt from the golden event log differs from "
        "golden-projection.json — the projection rules changed without "
        "regenerating the fixture (uv run python "
        "scripts/generate_event_store_fixture.py)"
    )


def test_golden_db_request_state_is_empty_before_rebuild(tmp_path):
    """The fixture must prove the projection is derived, not stored."""
    _load_db_in(tmp_path)
    from symeraseme.core.db_connection import get_connection

    conn = get_connection()
    rows = conn.execute("SELECT COUNT(*) FROM request_state").fetchone()[0]
    assert rows == 0
    with open(PROJECTION_FIXTURE) as f:
        expected = json.load(f)
    assert len(expected) >= 3, "fixture must cover several lifecycle paths"


def test_golden_db_covers_all_three_lifecycle_paths(tmp_path):
    _load_db_in(tmp_path)
    with open(PROJECTION_FIXTURE) as f:
        expected = json.load(f)
    statuses = {v["current_status"] for v in expected.values()}
    assert "CONFIRMED" in statuses  # happy path closes
    assert "REJECTED_FINAL" in statuses  # rejection path closes
    assert "ESCALATED" in statuses  # escalation path escalates
    # Deadline derived from SENT + expected_response_days payload.
    r1 = expected["1"]
    assert r1["deadline_at"] == "2026-08-31T08:05:00+00:00"
    # Reminder counter from REMINDER_SENT payload count.
    r3 = next(v for v in expected.values() if v["current_status"] == "ESCALATED")
    assert r3["reminders_sent"] == 1
    assert r3["escalation_level"] == 2
