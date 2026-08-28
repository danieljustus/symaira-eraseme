"""Generate the golden template renders for the Go port's #716 conformance test.

Renders EVERY template in registry/laws + registry/templates with the REAL
Python jinja2 environment (same flags as production: trim_blocks,
lstrip_blocks, HTML autoescape) against a fixed identity profile and fixed
vars, and dumps the rendered output for the Go text/template port to
replay byte-identically.

Usage: .venv/bin/python scripts/gen_golden_templates.py
"""
from __future__ import annotations

import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> int:
    sys.path.insert(0, os.path.join(REPO, "src"))
    from symeraseme.core.templating import list_templates, render_template
    from symeraseme.registry.schema import Address, IdentityProfile

    profile = IdentityProfile(
        full_name="Max Mustermann",
        name_variants=["Max M.", "M. Mustermann"],
        date_of_birth="1990-06-15",
        addresses=[
            Address(
                street="Teststrasse 1",
                city="Berlin",
                postal_code="10115",
                country="DE",
            ),
            Address(
                street="Alte Adresse 2",
                city="Muenchen",
                postal_code="80331",
                country="DE",
            ),
        ],
        email_addresses=["max@example.com", "max.m@example.org"],
        phone_numbers=["+49 30 123456"],
        jurisdictions=["DE", "EU"],
    )
    extra = {
        "original_request_date": "2026-07-20",
        "request_id": "REQ-12345",
        "broker_reply_snippet": "We could not verify your identity.",
        "auto_refresh_seconds": 0,
        "data": {
            "total_requests": 3,
            "planned": 1,
            "sent": 1,
            "awaiting_ack": 0,
            "awaiting_response": 0,
            "confirmed": 1,
            "rejected": 0,
            "overdue": 0,
            "campaigns": [
                {
                    "campaign_id": "q3-2026",
                    "id": "q3-2026",
                    "kind": "initial",
                    "created_at": "2026-07-01T09:00:00+00:00",
                    "total": 3,
                    "confirmation_rate": 33,
                    "planned": 1,
                    "sent": 1,
                    "awaiting_ack": 0,
                    "awaiting_response": 0,
                    "confirmed": 1,
                    "rejected": 0,
                    "overdue": 0,
                    "total_reminders_sent": 4,
                    "avg_response_time_days": 12,
                    "requests": [
                        {
                            "id": 1,
                            "broker_id": "acxiom-eu",
                            "jurisdiction": "DE",
                            "current_status": "CONFIRMED",
                            "sent_at": "2026-07-02T08:00:00+00:00",
                            "resolved_at": "2026-07-20T10:30:00+00:00",
                            "reminders_sent": 2,
                        },
                        {
                            "id": 2,
                            "broker_id": "oracle-data",
                            "jurisdiction": "US",
                            "current_status": "PLANNED",
                            "sent_at": "",
                            "resolved_at": "",
                            "reminders_sent": 0,
                        },
                    ],
                }
            ],
            "broker_status": [
                {
                    "broker_id": "acxiom-eu",
                    "total": 2,
                    "confirmed": 1,
                    "pending": 0,
                    "overdue": 0,
                    "rejected": 0,
                }
            ],
            "recent_events": [
                {
                    "occurred_at": "2026-07-20T10:30:00+00:00",
                    "event_type": "CONFIRMED",
                    "request_id": 1,
                    "broker_id": "acxiom-eu",
                    "source": "inbox",
                },
                {
                    "occurred_at": "2026-07-02T08:00:00+00:00",
                    "event_type": "SENT",
                    "request_id": 1,
                    "broker_id": "acxiom-eu",
                    "source": "system",
                },
            ],
            "success_metrics": {
                "overall_confirmation_rate": 33,
                "overall_rejection_rate": 0,
                "overdue_rate": 0,
                "avg_response_time_days": 12,
                "median_response_time_days": 12,
            },
            "historical_comparison": {
                "requests_change": "+10",
                "confirmation_rate_change": 5,
                "rejection_rate_change": -2,
            },
            "broker_leaderboard": [
                {
                    "broker_id": "acxiom-eu",
                    "total": 3,
                    "confirmed": 2,
                    "rejected": 0,
                    "overdue": 0,
                    "success_rate": 66,
                    "avg_response_time_days": 12,
                }
            ],
            "jurisdiction_stats": [
                {
                    "jurisdiction": "DE",
                    "total": 2,
                    "confirmed": 2,
                    "rejected": 0,
                    "overdue": 0,
                    "confirmation_rate": 100,
                }
            ],
            "timeline": [
                {"date": "2026-07-20", "total_events": 3, "events": ["ACK", "CONFIRMED", "NOTE_ADDED"]}
            ],
        },
    }
    # strftime needs a real datetime for the report footer; pass as extra var
    from datetime import UTC, datetime

    extra["now"] = datetime(2026, 8, 27, 14, 30, tzinfo=UTC)

    out = {}
    # Letters (registry/laws) — rendered with profile + broker context
    # exactly like execution.py does.
    for name in list_templates():
        rendered = render_template(
            name,
            profile=profile,
            broker_name="Acme Data Corp",
            broker_website="https://acme.example.com",
            extra_vars={
                "original_request_date": extra["original_request_date"],
                "request_id": extra["request_id"],
                "broker_reply_snippet": extra["broker_reply_snippet"],
            },
        )
        out["laws/" + name] = rendered

    # HTML reports (registry/templates) — rendered like dashboard/report.
    # render_template loads from registry/laws by default; pass the
    # templates dir explicitly so the HTML tree is exercised.
    tmpl_dir = os.path.join(REPO, "registry", "templates")
    dashboard = render_template(
        "dashboard.html.j2",
        templates_dir=tmpl_dir,
        extra_vars={"auto_refresh_seconds": 0, "data": extra["data"], "now": extra["now"]},
    )
    out["templates/dashboard.html.j2"] = dashboard

    report = render_template(
        "report.html.j2",
        templates_dir=tmpl_dir,
        extra_vars={"data": extra["data"], "now": extra["now"]},
    )
    out["templates/report.html.j2"] = report

    path = os.path.join(REPO, "tests", "fixtures", "event-store", "golden-templates.json")
    with open(path, "w") as f:
        json.dump(out, f, indent=2, sort_keys=True, ensure_ascii=False)
    print(f"WROTE {path} with {len(out)} templates")
    for k in sorted(out):
        print(f"  {k}: {len(out[k])} chars")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())