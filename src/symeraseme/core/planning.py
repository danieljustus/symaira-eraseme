"""Campaign planning: broker selection, request creation, and plan queries."""

from __future__ import annotations

import logging
from typing import Any

from symeraseme.core.events import create_campaign, create_removal_request, list_removal_requests
from symeraseme.core.identity import hash_profile, load_profile
from symeraseme.core.projection import append_event_and_project
from symeraseme.registry.loader import load_all_brokers
from symeraseme.registry.schema import Broker, EmailOptOut, WebFormOptOut

logger = logging.getLogger(__name__)


def plan_campaign(
    *,
    campaign_id: str,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    max_brokers: int = 30,
    notes: str | None = None,
) -> dict[str, Any]:
    """Scan registry, create PLANNED events for matching brokers."""
    logger.debug("Planning campaign %s (jurisdiction=%s law=%s)", campaign_id, jurisdiction, law)
    if not create_campaign(campaign_id, kind="initial", notes=notes):
        logger.warning(
            "Campaign '%s' already exists — appending new removal requests to "
            "the existing campaign.",
            campaign_id,
        )

    brokers = load_all_brokers(
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
    )

    try:
        profile = load_profile()
        identity_hash = hash_profile(profile)
    except FileNotFoundError:
        identity_hash = ""

    channels: list[tuple[Broker, dict[str, Any]]] = []
    for broker in brokers:
        ch = _select_channel(broker)
        if ch is not None:
            channels.append((broker, ch))

    if max_brokers and len(channels) > max_brokers:
        channels = channels[:max_brokers]

    planned: list[dict[str, Any]] = []
    for broker, channel in channels:
        template_id = _resolve_template(channel)
        request_id = create_removal_request(
            broker_id=broker.id,
            channel=channel["type"],
            campaign_id=campaign_id,
            jurisdiction=_resolve_jurisdiction(broker, jurisdiction),
            template_id=template_id,
            identity_snapshot_hash=identity_hash,
        )
        append_event_and_project(
            request_id,
            "PLANNED",
            payload={
                "broker_name": broker.name,
                "broker_website": broker.website,
                "channel": channel["type"],
                "endpoint": channel.get("endpoint", ""),
                "template": template_id,
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
                "template": template_id,
            }
        )

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


def _select_channel(broker: Broker) -> dict[str, Any] | None:
    for channel in broker.opt_out:
        if isinstance(channel, EmailOptOut):
            return {
                "type": "email",
                "endpoint": channel.endpoint,
                "template": channel.template,
                "locale": getattr(channel, "locale", ""),
                "expected_response_days": channel.expected_response_days,
                "required_fields": channel.required_fields,
            }
        if isinstance(channel, WebFormOptOut):
            return {
                "type": "web_form",
                "endpoint": channel.url,
                "form_spec": channel.form_spec,
                "expected_response_days": 30,
            }
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
        parts = [template]
        if locale:
            parts.append(locale)
        parts.append("md.j2")
        return ".".join(parts)
    template_list = channel.get("template", [])
    if template_list:
        return template_list[0] if isinstance(template_list, list) else str(template_list)
    return ""
