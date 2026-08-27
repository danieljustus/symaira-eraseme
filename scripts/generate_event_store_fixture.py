#!/usr/bin/env python3
"""Generate the golden event-store fixture (issue #709).

Builds a small but complete campaign database through the real repository
layer (plan -> send -> reply -> deadline -> escalation -> close) and writes:

  tests/fixtures/event-store/golden-campaign.db   — committed SQLite file
  tests/fixtures/event-store/golden-projection.json — expected projection
      output (request_state content rebuilt from the event log)

The Go port replays the same file through its own projection and must
produce the identical JSON.

Notes:
- recorded_at is pinned to a fixed timestamp so the fixture file is
  deterministic; occurred_at values are explicit per event.
- request_state is intentionally EMPTY in the database: the assertion is
  that the projection can be rebuilt from the log alone.
"""

from __future__ import annotations

import json
import os
import shutil
import sqlite3
import tempfile
from pathlib import Path

from symeraseme.core.db_connection import close_connection, init_db
from symeraseme.core.events import append_event, create_campaign, create_removal_request
from symeraseme.core.projection import rebuild_state

FIXTURE_DIR = Path("tests/fixtures/event-store")
DB_TARGET = FIXTURE_DIR / "golden-campaign.db"
PROJECTION_TARGET = FIXTURE_DIR / "golden-projection.json"

# Fixed timestamps make the fixture reproducible. All times are UTC.
T = "2026-08-01T08:00:00"
RECORDED_AT = "2026-08-01T08:00:00"


def _pin_recorded_at(db: Path) -> None:
    conn = sqlite3.connect(db)
    conn.execute(
        "UPDATE request_events SET recorded_at = ? WHERE recorded_at != ?",
        (RECORDED_AT, RECORDED_AT),
    )
    # campaign/request creation timestamps default to datetime('now'); pin
    # them so regenerating the fixture is byte-stable.
    conn.execute(
        "UPDATE campaigns SET created_at = ? WHERE created_at != ?",
        (RECORDED_AT, RECORDED_AT),
    )
    conn.execute(
        "UPDATE removal_requests SET created_at = ? WHERE created_at != ?",
        (RECORDED_AT, RECORDED_AT),
    )
    conn.commit()
    conn.close()


def main() -> None:
    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        os.environ["SYMERASEME_DB_DIR"] = str(Path(tmp))
        close_connection()
        init_db()  # creates <tmp>/events.db via config.db_path
        from symeraseme.core.config import get_config

        work = get_config().db_path
        work = Path(work)

        create_campaign("golden-campaign", kind="initial", notes="golden fixture campaign")

        # Request 1 — happy path: PLANNED -> SENT -> ACK -> CONFIRMED.
        r1 = create_removal_request(
            broker_id="golden-webform-us",
            channel="web_form",
            campaign_id="golden-campaign",
            jurisdiction="US",
            template_id="ccpa-deletion",
            identity_snapshot_hash="sha256-0001",
        )
        append_event(
            r1,
            "PLANNED",
            payload={"campaign_id": "golden-campaign"},
            source="system",
            occurred_at="2026-08-01T08:00:00",
        )
        append_event(
            r1,
            "SENT",
            payload={"expected_response_days": 30, "broker_id": "golden-webform-us"},
            source="system",
            occurred_at="2026-08-01T08:05:00",
        )
        append_event(
            r1,
            "ACK",
            payload={"message_id": "msg-1001"},
            source="inbox",
            occurred_at="2026-08-03T09:12:00",
        )
        append_event(
            r1,
            "CONFIRMED",
            payload={"via": "ack"},
            source="user",
            occurred_at="2026-08-03T09:14:00",
        )

        # Request 2 — rejection path: PLANNED -> SENT -> REJECTED_FINAL.
        r2 = create_removal_request(
            broker_id="golden-email-eu",
            channel="email",
            campaign_id="golden-campaign",
            jurisdiction="DE",
            template_id="gdpr-art17",
            identity_snapshot_hash="sha256-0002",
        )
        append_event(
            r2,
            "PLANNED",
            payload={"campaign_id": "golden-campaign"},
            source="system",
            occurred_at="2026-08-01T08:00:00",
        )
        append_event(
            r2,
            "SENT",
            payload={"expected_response_days": 45, "broker_id": "golden-email-eu"},
            source="system",
            occurred_at="2026-08-01T08:06:00",
        )
        append_event(
            r2,
            "REJECTED_FINAL",
            payload={"reason": "no-legal-basis"},
            source="inbox",
            occurred_at="2026-08-12T14:00:00",
        )

        # Request 3 — escalation path: PLANNED -> SENT -> DEADLINE_REACHED
        #            -> REMINDER_SENT -> DPA_COMPLAINT_DRAFTED.
        r3 = create_removal_request(
            broker_id="golden-multi-uk",
            channel="email",
            campaign_id="golden-campaign",
            jurisdiction="UK",
            template_id="gdpr-art17",
            identity_snapshot_hash="sha256-0003",
        )
        append_event(
            r3,
            "PLANNED",
            payload={"campaign_id": "golden-campaign"},
            source="system",
            occurred_at="2026-08-01T08:00:00",
        )
        append_event(
            r3,
            "SENT",
            payload={"expected_response_days": 30, "broker_id": "golden-multi-uk"},
            source="system",
            occurred_at="2026-08-01T08:07:00",
        )
        append_event(
            r3,
            "DEADLINE_REACHED",
            payload={"expected_response_days": 30},
            source="scheduler",
            occurred_at="2026-09-01T08:00:00",
        )
        append_event(
            r3,
            "REMINDER_SENT",
            payload={"count": 1},
            source="scheduler",
            occurred_at="2026-09-02T08:00:00",
        )
        append_event(
            r3,
            "DPA_COMPLAINT_DRAFTED",
            payload={"authority": "ICO"},
            source="user",
            occurred_at="2026-09-10T10:00:00",
        )

        close_connection()

        # Projection output: rebuilt purely from the event log.
        projection: dict[str, object] = {}
        for rid in (r1, r2, r3):
            os.environ["SYMERASEME_DB_DIR"] = str(Path(tmp))
            close_connection()
            init_db()
            projection[str(rid)] = rebuild_state(rid)
        close_connection()

        _pin_recorded_at(work)
        shutil.copy2(work, DB_TARGET)
        (PROJECTION_TARGET).write_text(
            json.dumps(projection, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    print(f"wrote {DB_TARGET} and {PROJECTION_TARGET}")
    print(json.dumps(projection, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
