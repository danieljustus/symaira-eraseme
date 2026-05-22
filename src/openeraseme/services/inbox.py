from __future__ import annotations

import json

from openeraseme.adapters.email.smtp_imap import (
    IMAPError,
    match_reply_to_request,
)
from openeraseme.adapters.email.smtp_imap import (
    poll_inbox as _poll,
)
from openeraseme.core.db import init_db
from openeraseme.core.events import list_removal_requests
from openeraseme.core.orchestrator import submit_inbox_reply


def handle_poll_inbox(
    host: str,
    port: int,
    username: str,
    since_days: int,
    ssl: bool,
    campaign_id: str | None,
    password: str,
    output_format: str = "text",
) -> str:
    init_db()

    try:
        messages = _poll(
            host=host,
            port=port,
            username=username,
            password=password,
            ssl=ssl,
            since_days=since_days,
        )
    except IMAPError as e:
        import typer

        typer.echo(
            f"IMAP error: {e}. "
            "Check your credentials, ensure IMAP is enabled, and use an app password if 2FA is on.",
            err=True,
        )
        raise typer.Exit(1) from e

    if messages:
        requests = list_removal_requests(campaign_id=campaign_id)
        matched = match_reply_to_request(messages, requests)
        for msg in matched:
            submit_inbox_reply(
                msg.get("message_id", ""),
                request_id=msg.get("request_id"),
                from_addr=msg.get("from_addr", ""),
                subject=msg.get("subject", ""),
                snippet=msg.get("body", "")[:200],
            )
    else:
        matched = []

    if output_format == "json":
        output = {
            "total_fetched": len(messages),
            "total_matched": sum(1 for m in matched if m.get("request_id") is not None),
            "messages": matched,
        }
        return json.dumps(output, indent=2, default=str)

    lines = [f"Fetched {len(messages)} messages from inbox"]
    matched_count = sum(1 for m in matched if m.get("request_id") is not None)
    lines.append(f"Matched to requests: {matched_count}")
    for m in matched:
        req_id = m.get("request_id", "unmatched")
        lines.append(f"  [{req_id}] {m.get('subject', '(no subject)')}")

    if not messages:
        lines.append("No new messages found.")

    return "\n".join(lines)
