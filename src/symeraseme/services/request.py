from __future__ import annotations

import json

from symeraseme.core.db import init_db
from symeraseme.core.events import get_events, list_removal_requests


def handle_events_show(request_id: int, output_format: str = "text") -> str:
    init_db()
    events = get_events(request_id)

    if output_format == "json":
        return json.dumps(events, indent=2, default=str)

    if not events:
        return f"No events found for request #{request_id}"

    lines = [f"Events for request #{request_id}:"]
    for e in events:
        lines.append(f"  #{e['id']} {e['event_type']} @ {e['occurred_at']} (source: {e['source']})")
    return "\n".join(lines)


def handle_requests_list(
    campaign_id: str | None = None,
    status: str | None = None,
    broker_id: str | None = None,
    page: int | None = None,
    page_size: int = 250,
    output_format: str = "text",
) -> str:
    init_db()
    limit = page_size if page is not None else None
    offset = (page - 1) * page_size if page is not None else None
    requests = list_removal_requests(
        campaign_id=campaign_id,
        status=status,
        broker_id=broker_id,
        limit=limit,
        offset=offset,
    )

    if output_format == "json":
        return json.dumps(requests, indent=2, default=str)

    if not requests:
        return "No requests found."

    lines = []
    for r in requests:
        lines.append(
            f"  #{r['id']} [{r.get('current_status', 'N/A')}] {r['broker_id']} ({r['campaign_id']})"
        )
    return "\n".join(lines)
