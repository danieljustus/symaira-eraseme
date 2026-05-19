"""Plan, execute, and poll orchestration for removal campaigns."""

from __future__ import annotations

from typing import Any

from openeraseme.core.events import (
    append_event,
    create_campaign,
    create_removal_request,
    get_events,
    get_removal_request,
    list_removal_requests,
)
from openeraseme.core.projection import rebuild_all_states, upsert_state
from openeraseme.registry.loader import load_all_brokers
from openeraseme.registry.schema import Broker


def plan_campaign(
    *,
    campaign_id: str,
    jurisdiction: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    max_brokers: int = 30,
    notes: str | None = None,
) -> dict[str, Any]:
    """Scan registry, create PLANNED events for matching brokers."""
    create_campaign(campaign_id, kind="initial", notes=notes)

    brokers = load_all_brokers(
        jurisdiction=jurisdiction,
        priority=priority,
        category=category,
    )

    if max_brokers and len(brokers) > max_brokers:
        brokers = brokers[:max_brokers]

    planned: list[dict[str, Any]] = []
    for broker in brokers:
        channel = _select_channel(broker)
        if not channel:
            continue

        request_id = create_removal_request(
            broker_id=broker.id,
            channel=channel["type"],
            campaign_id=campaign_id,
            jurisdiction=_resolve_jurisdiction(broker, jurisdiction),
            template_id=_resolve_template(channel),
            identity_snapshot_hash="",
        )
        resolved_template = _resolve_template(channel)
        append_event(
            request_id,
            "PLANNED",
            payload={
                "broker_name": broker.name,
                "broker_website": broker.website,
                "channel": channel["type"],
                "endpoint": channel.get("endpoint", ""),
                "template": resolved_template,
                "locale": channel.get("locale", ""),
                "expected_response_days": channel.get("expected_response_days", 30),
            },
        )
        planned.append(
            {
                "request_id": request_id,
                "broker_id": broker.id,
                "broker_name": broker.name,
                "channel": channel["type"],
                "template": resolved_template,
            }
        )

    rebuild_all_states()
    return {
        "campaign_id": campaign_id,
        "total_brokers": len(brokers),
        "planned": len(planned),
        "requests": planned,
    }


def get_plan(
    campaign_id: str | None = None,
    *,
    status: str | None = None,
) -> dict[str, Any]:
    requests = list_removal_requests(campaign_id=campaign_id, status=status)
    return {
        "campaign_id": campaign_id or "all",
        "total": len(requests),
        "requests": requests,
    }


def execute_request(
    request_id: int,
    *,
    account: str | None = None,
    config_path: str | None = None,
    dry_run: bool = False,
) -> dict[str, Any]:
    """Execute a single removal request by sending an email via Himalaya."""
    req = get_removal_request(request_id)
    if req is None:
        return {"success": False, "error": f"Request {request_id} not found"}

    from openeraseme.adapters.email.himalaya import HimalayaError, send_message
    from openeraseme.core.templating import render_template

    broker_name = req["broker_id"]
    events = get_events(request_id)
    last_event = events[-1] if events else {}
    payload = last_event.get("payload_json", {})

    channel_endpoint = payload.get("endpoint", "")
    template_id = req.get("template_id", "")

    if dry_run:
        rendered = render_template(
            template_id,
            broker_name=broker_name,
        )
        return {
            "success": True,
            "dry_run": True,
            "request_id": request_id,
            "to": channel_endpoint,
            "subject": f"Data Deletion Request — {broker_name}",
            "body": rendered,
        }

    try:
        rendered = render_template(
            template_id,
            broker_name=broker_name,
        )
        result = send_message(
            to=channel_endpoint,
            subject=f"Data Deletion Request — {broker_name}",
            body=rendered,
            account=account,
            config_path=config_path,
        )
        append_event(
            request_id,
            "SENT",
            payload={
                "to": channel_endpoint,
                "template": template_id,
                "account": account or "",
                "expected_response_days": payload.get("expected_response_days", 30),
            },
        )
        upsert_state(request_id)
        return {"success": True, "request_id": request_id, "result": result}
    except HimalayaError as e:
        append_event(
            request_id,
            "SEND_FAILED",
            payload={"error": str(e), "to": channel_endpoint},
        )
        upsert_state(request_id)
        return {"success": False, "error": str(e), "request_id": request_id}


def execute_campaign(
    campaign_id: str,
    *,
    account: str | None = None,
    config_path: str | None = None,
    batch_size: int = 5,
    dry_run: bool = False,
) -> dict[str, Any]:
    requests = list_removal_requests(campaign_id=campaign_id, status="PLANNED")
    batch = requests[:batch_size]

    results: list[dict[str, Any]] = []
    for req in batch:
        result = execute_request(
            req["id"],
            account=account,
            config_path=config_path,
            dry_run=dry_run,
        )
        results.append(result)

    return {
        "campaign_id": campaign_id,
        "total_planned": len(requests),
        "batch_size": len(batch),
        "results": results,
    }


def _select_channel(broker: Broker) -> dict[str, Any] | None:
    from openeraseme.registry.schema import EmailOptOut

    for channel in broker.opt_out:
        if isinstance(channel, EmailOptOut):
            return {
                "type": "email",
                "endpoint": channel.endpoint,
                "template": channel.template,
                "locale": getattr(channel, "locale", ""),
                "expected_response_days": channel.expected_response_days,
            }
    if broker.opt_out:
        ch = broker.opt_out[0]
        return {"type": ch.type}
    return None


def _resolve_jurisdiction(broker: Broker, requested: str | None) -> str:
    if requested and requested in broker.jurisdictions:
        return requested
    if broker.jurisdictions:
        return broker.jurisdictions[0]
    return "UNKNOWN"


def _resolve_template(channel: dict[str, Any]) -> str:
    template = channel.get("template", "")
    locale = channel.get("locale", "")
    if isinstance(template, str) and template:
        # Build the full filename: gdpr-art17 + (locale) + .md.j2
        parts = [template]
        if locale:
            parts.append(locale)
        parts.append("md.j2")
        return ".".join(parts)
    template_list = channel.get("template", [])
    if template_list:
        return template_list[0] if isinstance(template_list, list) else str(template_list)
    return ""


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
    from openeraseme.core.db import get_connection

    conn = get_connection()
    cur = conn.execute(
        """INSERT OR IGNORE INTO inbox_replies
           (request_id, message_id, thread_id, received_at, from_addr, subject,
            snippet, classified_as)
           VALUES (?, ?, ?, datetime('now'), ?, ?, ?, ?)""",
        (request_id, message_id, thread_id, from_addr, subject, snippet, classified_as),
    )
    conn.commit()
    reply_id = cur.lastrowid

    if request_id and classified_as:
        _event_for_classification(request_id, classified_as, subject, from_addr)
        upsert_state(request_id)

    return {"reply_id": reply_id, "request_id": request_id, "classified_as": classified_as}


def _event_for_classification(
    request_id: int,
    classified_as: str,
    subject: str,
    from_addr: str,
) -> None:
    mapping: dict[str, str] = {
        "ack": "ACK",
        "verification": "VERIFICATION_REQUESTED",
        "confirmed": "CONFIRMED",
        "rejected": "REJECTED_FINAL",
        "human_required": "HUMAN_ACTION_REQUIRED",
        "autoresponder": "AUTORESPONDER",
        "bounce": "BOUNCE",
        "noise": "NOTE_ADDED",
    }
    event_type = mapping.get(classified_as, "NOTE_ADDED")
    append_event(
        request_id,
        event_type,
        payload={"subject": subject, "from": from_addr},
        source="inbox",
    )
