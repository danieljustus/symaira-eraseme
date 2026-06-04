"""Repository layer for centralized SQL queries."""

from __future__ import annotations

from symeraseme.core.repositories.campaigns import create_campaign, list_campaigns
from symeraseme.core.repositories.dashboard import (
    fetch_campaigns,
    fetch_recent_events,
    fetch_requests_for_campaigns,
)
from symeraseme.core.repositories.deadlines import fetch_tick_candidates
from symeraseme.core.repositories.events import append_event, get_events, get_events_for_requests
from symeraseme.core.repositories.inbox import insert_inbox_reply
from symeraseme.core.repositories.manual_tasks import (
    get_manual_task,
    insert_manual_task,
    list_manual_tasks,
    update_manual_task_status,
)
from symeraseme.core.repositories.requests import (
    create_removal_request,
    get_removal_request,
    list_removal_requests,
)

__all__ = [
    "create_campaign",
    "list_campaigns",
    "create_removal_request",
    "get_removal_request",
    "list_removal_requests",
    "append_event",
    "get_events",
    "get_events_for_requests",
    "fetch_campaigns",
    "fetch_recent_events",
    "fetch_requests_for_campaigns",
    "fetch_tick_candidates",
    "insert_inbox_reply",
    "get_manual_task",
    "insert_manual_task",
    "list_manual_tasks",
    "update_manual_task_status",
]
