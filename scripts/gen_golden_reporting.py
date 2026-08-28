"""Generate deterministic report/dashboard/calendar fixtures from Python."""
from __future__ import annotations

import json
import os
import sqlite3
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import patch

FIXED = datetime(2026, 8, 6, 12, 0, tzinfo=UTC)


class FixedDateTime(datetime):
    @classmethod
    def now(cls, tz=None):
        return FIXED if tz else FIXED.replace(tzinfo=None)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    data_dir = Path(tempfile.mkdtemp(prefix="eraseme-report-golden-"))
    os.environ["SYMERASEME_DB_DIR"] = str(data_dir)
    from symeraseme.core.db_connection import close_connection, init_db

    close_connection()
    init_db()
    db = sqlite3.connect(data_dir / "symeraseme.db")
    statements = [
        "INSERT INTO campaigns(id,created_at,kind,notes) VALUES ('new','2026-08-02T08:00:00+00:00','initial','new'),('old','2026-07-01T08:00:00+00:00','initial','old')",
        """INSERT INTO removal_requests(id,broker_id,channel,campaign_id,created_at,jurisdiction,template_id,identity_snapshot_hash) VALUES
        (1,'broker-a','email','new','2026-08-02T08:00:00+00:00','GDPR','gdpr','h'),
        (2,'broker-b','web_form','new','2026-08-02T09:00:00+00:00','CCPA','ccpa','h'),
        (3,'broker-a','email','old','2026-07-01T08:00:00+00:00','GDPR','gdpr','h')""",
        """INSERT INTO request_state(request_id,current_status,sent_at,resolved_at,deadline_at,next_action_at,reminders_sent,escalation_level,last_event_id,last_event_at) VALUES
        (1,'CONFIRMED','2026-08-02T08:00:00+00:00','2026-08-04T08:00:00+00:00','2026-09-01T08:00:00+00:00',NULL,1,0,2,'2026-08-04T08:00:00+00:00'),
        (2,'OVERDUE','2026-08-02T09:00:00+00:00',NULL,'2026-08-03T09:00:00+00:00','2026-08-05T09:00:00+00:00',2,2,4,'2026-08-05T09:00:00+00:00'),
        (3,'REJECTED_FINAL','2026-07-01T08:00:00+00:00','2026-07-03T08:00:00+00:00','2026-07-31T08:00:00+00:00',NULL,0,0,6,'2026-07-03T08:00:00+00:00')""",
        """INSERT INTO request_events(id,request_id,event_type,occurred_at,payload_json,source) VALUES
        (1,1,'SENT','2026-08-02T08:00:00+00:00','{}','system'),(2,1,'CONFIRMED','2026-08-04T08:00:00+00:00','{}','inbox'),
        (3,2,'SENT','2026-08-02T09:00:00+00:00','{}','system'),(4,2,'DEADLINE_REACHED','2026-08-05T09:00:00+00:00','{}','scheduler'),
        (5,3,'SENT','2026-07-01T08:00:00+00:00','{}','system'),(6,3,'REJECTED_FINAL','2026-07-03T08:00:00+00:00','{}','inbox')""",
    ]
    for statement in statements:
        db.execute(statement)
    db.commit()
    db.close()
    close_connection()

    from symeraseme.core import dashboard
    from symeraseme.core.reports import data

    with patch.object(data, "datetime", FixedDateTime), patch.object(dashboard, "datetime", FixedDateTime):
        fixture = {
            "report": data.get_report_data(all_campaigns=True),
            "dashboard": dashboard.get_dashboard_data(),
            "campaign_status": data.get_campaign_status(),
            "calendar": data.get_calendar_entries(weeks=4),
        }
    target = root / "tests" / "fixtures" / "event-store" / "golden-reporting.json"
    target.write_text(json.dumps(fixture, indent=2, sort_keys=True, ensure_ascii=False) + "\n")
    print(target)


if __name__ == "__main__":
    main()
