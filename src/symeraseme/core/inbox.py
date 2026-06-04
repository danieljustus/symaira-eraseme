"""Inbox reply storage and event recording."""

from __future__ import annotations

from typing import Any


def submit_inbox_reply(
    message_id: str,
    *,
    request_id: int | None = None,
    thread_id: str | None = None,
    from_addr: str = "",
    subject: str = "",
    snippet: str = "",
    classified_as: str | None = None,
) -> dict[str, Any]:
    """Store an inbox reply and append corresponding event."""
    from symeraseme.core.repositories.inbox import insert_inbox_reply

    reply_id = insert_inbox_reply(
        request_id=request_id,
        message_id=message_id,
        thread_id=thread_id,
        from_addr=from_addr,
        subject=subject,
        snippet=snippet,
        classified_as=classified_as,
    )

    return {"reply_id": reply_id, "request_id": request_id, "classified_as": classified_as}
