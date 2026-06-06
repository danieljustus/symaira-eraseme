from __future__ import annotations

import logging

from symeraseme.adapters.email.smtp_imap import (
    IMAPError,
    match_reply_to_request,
)
from symeraseme.adapters.email.smtp_imap import (
    poll_inbox as _poll,
)
from symeraseme.core.db import init_db
from symeraseme.core.events import get_events_for_requests, list_removal_requests
from symeraseme.core.inbox import submit_inbox_reply
from symeraseme.core.result_types import CliResult

logger = logging.getLogger(__name__)


def handle_poll_inbox(
    host: str,
    port: int,
    username: str,
    since_days: int,
    ssl: bool,
    campaign_id: str | None,
    password: str,
) -> CliResult:
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
        logger.debug("IMAP poll failed", exc_info=True)
        return CliResult(
            success=False,
            error=(
                f"IMAP error: {e}. "
                "Check your credentials, ensure IMAP is enabled, "
                "and use an app password if 2FA is on."
            ),
        )

    if messages:
        requests = list_removal_requests(campaign_id=campaign_id)
        thread_map: dict[str, int] = {}
        req_ids = []
        for req in requests:
            req_id = req.get("id") or req.get("request_id")
            if req_id:
                req_ids.append(req_id)
        if req_ids:
            events_by_rid = get_events_for_requests(req_ids)
            for rid, evs in events_by_rid.items():
                for ev in evs:
                    if ev.get("event_type") == "SENT":
                        payload = ev.get("payload_json", {})
                        msg_id = payload.get("message_id", "") if isinstance(payload, dict) else ""
                        if msg_id:
                            thread_map[msg_id] = rid

        matched = match_reply_to_request(messages, requests, thread_map)
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

    matched_count = sum(1 for m in matched if m.get("request_id") is not None)
    result = {
        "total_fetched": len(messages),
        "total_matched": matched_count,
        "messages": matched,
    }

    lines = [f"Fetched {len(messages)} messages from inbox"]
    lines.append(f"Matched to requests: {matched_count}")
    for m in matched:
        req_id = m.get("request_id", "unmatched")
        lines.append(f"  [{req_id}] {m.get('subject', '(no subject)')}")

    if not messages:
        lines.append("No new messages found.")

    result["message"] = "\n".join(lines)
    return CliResult(success=True, data=result)
