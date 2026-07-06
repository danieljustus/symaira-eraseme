"""MCP dashboard service handlers for SwiftUI macOS GUI data queries.

All handlers return ``CliResult`` and use lazy imports for optional extras
following the established convention in other service modules.
"""

from __future__ import annotations

import json
from typing import Any

from symeraseme.core.result_types import CliResult


def handle_get_dashboard_data() -> CliResult:
    """Return aggregated dashboard data from the event store.

    Returns a ``CliResult`` whose ``data`` dict contains campaigns,
    request status counts, broker status, and recent events.
    """
    from symeraseme.core.dashboard import get_dashboard_data

    try:
        data = get_dashboard_data()
        return CliResult(success=True, data=data)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to fetch dashboard data: {exc}")


def handle_list_requests(
    campaign_id: str | None = None,
    status: str | None = None,
    broker_id: str | None = None,
    page: int = 1,
    page_size: int = 100,
) -> CliResult:
    """Return paginated removal requests with optional filters.

    Args:
        campaign_id: Filter by campaign identifier.
        status: Filter by request status (e.g. PLANNED, SENT, CONFIRMED).
        broker_id: Filter by broker identifier.
        page: 1-indexed page number.
        page_size: Maximum items per page.

    Returns a ``CliResult`` with ``data`` containing ``page``, ``page_size``,
    ``total``, and ``items`` keys.
    """
    from symeraseme.core.repositories import count_removal_requests, list_removal_requests

    try:
        total = count_removal_requests(
            campaign_id=campaign_id,
            status=status,
        )
        offset = (max(page, 1) - 1) * page_size
        items = list_removal_requests(
            campaign_id=campaign_id,
            status=status,
            broker_id=broker_id,
            limit=page_size,
            offset=offset,
        )
        result: dict[str, Any] = {
            "page": page,
            "page_size": page_size,
            "total": total,
            "items": json.loads(json.dumps(items, default=str)),
        }
        return CliResult(success=True, data=result)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to list requests: {exc}")


def handle_get_events(
    request_id: int,
    after_event_id: int | None = None,
) -> CliResult:
    """Return the event history for a removal request.

    Args:
        request_id: The removal request ID to fetch events for.
        after_event_id: If provided, only return events with ID greater than this.

    Returns a ``CliResult`` with ``data`` containing ``request_id`` and
    ``events`` (list of event dicts with parsed payloads).
    """
    from symeraseme.core.repositories import get_events

    try:
        events = get_events(request_id, after_event_id=after_event_id)
        result: dict[str, Any] = {
            "request_id": request_id,
            "events": json.loads(json.dumps(events, default=str)),
        }
        return CliResult(success=True, data=result)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to get events: {exc}")


def handle_get_calendar(
    weeks: int = 4,
    campaign_id: str | None = None,
) -> CliResult:
    """Return upcoming deadlines and tick actions for the next N weeks.

    This is a **read-only** operation — ``run_tick`` is called with
    ``dry_run=True`` so no state is mutated.

    Args:
        weeks: Number of weeks to look ahead (default 4).
        campaign_id: Optional campaign filter for status queries.

    Returns a ``CliResult`` with ``data`` containing ``upcoming_deadlines``
    (from campaign status) and ``tick_actions`` (dry-run tick results).
    """
    from symeraseme.core.deadlines import run_tick
    from symeraseme.core.reports.data import get_campaign_status

    try:
        status = get_campaign_status(campaign_id=campaign_id)
        tick_actions = run_tick(dry_run=True)
        tick_dicts = [
            {
                "request_id": a.request_id,
                "broker_id": a.broker_id,
                "campaign_id": a.campaign_id,
                "current_status": a.current_status,
                "action_type": a.action_type,
                "event_type": a.event_type,
                "description": a.description,
                "payload": a.payload,
                "dry_run": a.dry_run,
            }
            for a in tick_actions
        ]
        result: dict[str, Any] = {
            "upcoming_deadlines": status,
            "tick_actions": tick_dicts,
            "weeks": weeks,
        }
        return CliResult(success=True, data=result)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to build calendar: {exc}")


def handle_list_brokers(
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> CliResult:
    """Return filtered brokers from the registry as a list of dicts.

    Args:
        jurisdiction: Filter by jurisdiction name (e.g. GDPR, CCPA).
        law: Filter by specific law.
        priority: Filter by priority level (high, medium, low).
        category: Filter by broker category (people-search, marketing, etc.).
        include_disabled: If True, include brokers marked as disabled.

    Returns a ``CliResult`` with ``data`` containing ``brokers`` (list of
    serialized Broker dicts) and ``total`` count.
    """
    from symeraseme.registry.loader import load_all_brokers

    try:
        brokers = load_all_brokers(
            jurisdiction=jurisdiction,
            law=law,
            priority=priority,
            category=category,
            include_disabled=include_disabled,
        )
        broker_dicts = [b.model_dump(mode="json") for b in brokers]
        result: dict[str, Any] = {
            "brokers": broker_dicts,
            "total": len(broker_dicts),
        }
        return CliResult(success=True, data=result)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to list brokers: {exc}")


def handle_get_profile() -> CliResult:
    """Return the decrypted identity profile as JSON.

    If no profile exists, returns a failed ``CliResult`` with an
    appropriate error message.  No PII redaction is applied — the
    GUI layer decides how to display sensitive data.

    Returns a ``CliResult`` with ``data`` containing the full
    ``IdentityProfile`` fields, or ``success=False`` if missing.
    """
    from symeraseme.core.identity import load_profile, profile_exists

    if not profile_exists():
        return CliResult(success=False, error="No profile found")

    try:
        profile = load_profile()
        data = json.loads(profile.model_dump_json())
        return CliResult(success=True, data=data)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to load profile: {exc}")


def handle_export_data(
    format: str = "json",
    campaign_id: str | None = None,
) -> CliResult:
    """Export report data without writing to a file.

    Returns the report data serialized as JSON in the ``CliResult``.

    Args:
        format: Export format — currently only ``"json"`` is supported.
        campaign_id: Optional campaign filter.

    Returns a ``CliResult`` with ``data`` containing the report
    payload (campaigns, status breakdown, broker leaderboard, etc.).
    """
    from symeraseme.core.reports.data import get_report_data

    try:
        report = get_report_data(campaign_id=campaign_id)
        serialized = json.loads(json.dumps(report, default=str))
        result: dict[str, Any] = {
            "format": format,
            "data": serialized,
        }
        return CliResult(success=True, data=result)
    except Exception as exc:
        return CliResult(success=False, error=f"Failed to export data: {exc}")
