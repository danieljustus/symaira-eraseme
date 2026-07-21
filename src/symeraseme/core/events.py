"""Append-only event store for removal request lifecycle."""

from __future__ import annotations

import logging
from typing import Any

from symeraseme.core.repositories import (
    append_event as _repo_append_event,
)
from symeraseme.core.repositories import (
    create_campaign as _repo_create_campaign,
)
from symeraseme.core.repositories import (
    create_removal_request as _repo_create_removal_request,
)
from symeraseme.core.repositories import (
    get_events as _repo_get_events,
)
from symeraseme.core.repositories import (
    get_events_for_requests as _repo_get_events_for_requests,
)
from symeraseme.core.repositories import (
    get_removal_request as _repo_get_removal_request,
)
from symeraseme.core.repositories import (
    list_campaigns as _repo_list_campaigns,
)
from symeraseme.core.repositories import (
    list_removal_requests as _repo_list_removal_requests,
)

logger = logging.getLogger(__name__)

EVENT_TYPES = frozenset(
    {
        "PLANNED",
        "SENT",
        "SEND_FAILED",
        "BOUNCE",
        "AUTORESPONDER",
        "ACK",
        "VERIFICATION_REQUESTED",
        "VERIFICATION_PROVIDED",
        "HUMAN_ACTION_REQUIRED",
        "CONFIRMATION_LINK_CLICKED",
        "REPLY_DRAFTED",
        "REBUTTAL_SENT",
        "REMINDER_SENT",
        "DEADLINE_REACHED",
        "DPA_COMPLAINT_DRAFTED",
        "DPA_COMPLAINT_FILED",
        "CONFIRMED",
        "REJECTED_FINAL",
        "RE_SCAN_TRIGGERED",
        "NOTE_ADDED",
    }
)

VALID_SOURCES = frozenset({"system", "inbox", "user", "scheduler"})


def create_campaign(
    campaign_id: str,
    kind: str = "initial",
    notes: str | None = None,
) -> bool:
    return _repo_create_campaign(campaign_id, kind=kind, notes=notes)


def list_campaigns() -> list[dict[str, Any]]:
    return _repo_list_campaigns()


def create_removal_request(
    *,
    broker_id: str,
    channel: str = "email",
    campaign_id: str,
    jurisdiction: str,
    template_id: str = "",
    identity_snapshot_hash: str = "",
) -> int:
    return _repo_create_removal_request(
        broker_id=broker_id,
        channel=channel,
        campaign_id=campaign_id,
        jurisdiction=jurisdiction,
        template_id=template_id,
        identity_snapshot_hash=identity_snapshot_hash,
    )


def append_event(
    request_id: int,
    event_type: str,
    *,
    payload: dict[str, Any] | None = None,
    source: str = "system",
    occurred_at: str | None = None,
    commit: bool = True,
) -> int:
    """Append an event to the request_events log.

    When ``commit=False`` the caller is responsible for committing — used by
    ``append_event_and_project()`` to atomically bundle event + projection in
    a single transaction.
    """
    if event_type not in EVENT_TYPES:
        msg = f"Unknown event type: {event_type}. Valid: {sorted(EVENT_TYPES)}"
        raise ValueError(msg)
    if source not in VALID_SOURCES:
        msg = f"Unknown source: {source}. Valid: {sorted(VALID_SOURCES)}"
        raise ValueError(msg)

    logger.debug("Appending event %s for request %s", event_type, request_id)
    return _repo_append_event(
        request_id,
        event_type,
        payload=payload,
        source=source,
        occurred_at=occurred_at,
        commit=commit,
    )


def get_events_for_requests(
    request_ids: list[int],
    *,
    event_type: str | None = None,
) -> dict[int, list[dict[str, Any]]]:
    return _repo_get_events_for_requests(request_ids, event_type=event_type)


def get_events(
    request_id: int,
    *,
    after_event_id: int | None = None,
) -> list[dict[str, Any]]:
    return _repo_get_events(request_id, after_event_id=after_event_id)


def get_removal_request(request_id: int) -> dict[str, Any] | None:
    return _repo_get_removal_request(request_id)


def list_removal_requests(
    *,
    campaign_id: str | None = None,
    status: str | None = None,
    broker_id: str | None = None,
    limit: int | None = None,
    offset: int | None = None,
) -> list[dict[str, Any]]:
    return _repo_list_removal_requests(
        campaign_id=campaign_id,
        status=status,
        broker_id=broker_id,
        limit=limit,
        offset=offset,
    )
