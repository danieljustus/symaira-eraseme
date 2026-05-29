"""Plan, execute, and poll orchestration for removal campaigns."""

from __future__ import annotations

from collections import defaultdict
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from symeraseme.core.protocols import WebFormRunner

from symeraseme.core.events import (
    append_event,
    create_campaign,
    create_removal_request,
    get_events,
    get_events_for_requests,
    get_removal_request,
    list_removal_requests,
)
from symeraseme.core.identity import hash_profile, load_profile
from symeraseme.core.projection import append_event_and_project, rebuild_all_states
from symeraseme.registry.loader import load_all_brokers
from symeraseme.registry.schema import Broker

_BATCH_LIMIT = 10


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
    create_campaign(campaign_id, kind="initial", notes=notes)

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
        append_event(
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
    web_form_runner: WebFormRunner | None = None,
) -> dict[str, Any]:
    """Execute a single removal request by sending an email or running a web form."""
    req = get_removal_request(request_id)
    if req is None:
        return {"success": False, "error": f"Request {request_id} not found"}

    broker_name = req["broker_id"]
    events = get_events(request_id)
    last_event = events[-1] if events else {}
    payload = last_event.get("payload_json", {})

    channel_type = req.get("channel", "email")

    if channel_type == "web_form":
        if web_form_runner is None:
            from symeraseme.services.web_form import run_web_form_for_broker

            web_form_runner = run_web_form_for_broker

        result = web_form_runner(
            broker_name,
            dry_run=dry_run,
        )
        identity_hash = ""
        try:
            profile = load_profile()
            identity_hash = hash_profile(profile)
        except FileNotFoundError:
            pass
        if result["success"]:
            append_event_and_project(
                request_id,
                "SENT",
                payload={
                    "broker_name": broker_name,
                    "form_url": result.get("url", ""),
                    "expected_response_days": 30,
                    "identity_snapshot_hash": identity_hash,
                },
            )
        else:
            append_event_and_project(
                request_id,
                "SEND_FAILED",
                payload={
                    "error": result.get("error", ""),
                    "broker_name": broker_name,
                    "task_id": result.get("task_id"),
                },
            )
        return {"success": result["success"], "request_id": request_id, **result}

    from symeraseme.adapters.email.himalaya import EmailError, send_email
    from symeraseme.core.templating import render_template

    channel_endpoint = payload.get("endpoint", "")
    template_id = req.get("template_id", "")
    required_fields = payload.get("required_fields", ["full_name", "email_addresses"])

    try:
        profile = load_profile()
        identity_hash = hash_profile(profile)
    except FileNotFoundError:
        return {
            "success": False,
            "error": (
                "Identity profile not found. "
                "Run 'symeraseme init-profile' first to create your identity profile."
            ),
            "request_id": request_id,
        }

    missing = []
    profile_vars = profile.model_dump()
    for field in required_fields:
        value = profile_vars.get(field)
        if value is None or value == [] or value == {} or value == "":
            missing.append(field)
    if missing:
        return {
            "success": False,
            "error": (
                f"Missing required identity fields: {', '.join(missing)}. "
                f"Run 'symeraseme init-profile' to update your profile."
            ),
            "request_id": request_id,
        }

    if dry_run:
        rendered = render_template(
            template_id,
            profile=profile,
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
            profile=profile,
            broker_name=broker_name,
        )
        send_result = send_email(
            to=channel_endpoint,
            subject=f"Data Deletion Request — {broker_name}",
            body=rendered,
            account=account,
            config_path=config_path,
        )
        append_event_and_project(
            request_id,
            "SENT",
            payload={
                "to": channel_endpoint,
                "template": template_id,
                "account": account or "",
                "expected_response_days": payload.get("expected_response_days", 30),
                "message_id": send_result.get("message_id", ""),
                "identity_snapshot_hash": identity_hash,
            },
        )
        return {"success": True, "request_id": request_id, "result": send_result}
    except EmailError as e:
        append_event_and_project(
            request_id,
            "SEND_FAILED",
            payload={"error": str(e), "to": channel_endpoint},
        )
        return {"success": False, "error": str(e), "request_id": request_id}


def execute_campaign(
    campaign_id: str,
    *,
    account: str | None = None,
    config_path: str | None = None,
    batch_size: int = 5,
    dry_run: bool = False,
    web_form_runner: WebFormRunner | None = None,
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
            web_form_runner=web_form_runner,
        )
        results.append(result)

    return {
        "campaign_id": campaign_id,
        "total_planned": len(requests),
        "batch_size": len(batch),
        "results": results,
    }


async def execute_campaign_async(
    campaign_id: str,
    *,
    batch_size: int = _BATCH_LIMIT,
    dry_run: bool = False,
    smtp_skip_tls: bool = False,
    web_form_runner: WebFormRunner | None = None,
) -> dict[str, Any]:
    """Execute a campaign using direct SMTP for batched sending.

    Collects all PLANNED removal requests, renders email templates,
    and sends them over a single SMTP connection instead of spawning
    a Himalaya CLI process per message.

    Parameters
    ----------
    campaign_id : str
        The campaign to execute.
    batch_size : int
        Max number of messages to send (default 10).
    dry_run : bool
        When true, renders templates without sending.
    smtp_skip_tls : bool
        When true, disables STARTTLS (for testing with local SMTP
        servers that don't support TLS).

    Returns
    -------
    dict[str, Any]
        Campaign execution summary with results per request.
    """
    requests = list_removal_requests(campaign_id=campaign_id, status="PLANNED")
    batch = requests[:batch_size]

    from symeraseme.adapters.email.himalaya import (
        EmailMessage,
        SmtpConfig,
        load_smtp_config,
        send_messages_batch,
    )
    from symeraseme.core.templating import render_template

    # Load the identity profile once before the template-rendering loop so
    # that every rendered email contains the user's actual profile data
    # (full name, email addresses, phone numbers, addresses, etc.).
    try:
        profile = load_profile()
    except FileNotFoundError:
        profile = None

    if dry_run:
        results: list[dict[str, Any]] = []
        for req in batch:
            r = execute_request(req["id"], dry_run=True, web_form_runner=web_form_runner)
            results.append(r)
        return {
            "campaign_id": campaign_id,
            "total_planned": len(requests),
            "batch_size": len(batch),
            "results": results,
        }

    smtp_config = load_smtp_config()
    if smtp_skip_tls:
        smtp_config = SmtpConfig(
            host=smtp_config.host,
            port=smtp_config.port,
            username=smtp_config.username,
            password=smtp_config.password,
            use_tls=False,
            from_addr=smtp_config.from_addr,
        )

    email_messages: list[EmailMessage] = []
    # Multi-map: one endpoint may serve multiple requests (e.g. same broker
    # in different campaigns, or duplicate entries).  Store every req_id
    # and consume them in FIFO order when processing SMTP results.
    endpoint_ids: defaultdict[str, list[int]] = defaultdict(list)

    batch_ids = [r["id"] for r in batch]
    events_by_rid = get_events_for_requests(batch_ids) if batch_ids else {}

    for req in batch:
        req_id = req["id"]
        broker_name = req["broker_id"]
        events = events_by_rid.get(req_id, [])
        last_event = events[-1] if events else {}
        payload = last_event.get("payload_json", {}) if isinstance(last_event, dict) else {}
        channel_endpoint = payload.get("endpoint", "")
        template_id = req.get("template_id", "")

        if not channel_endpoint:
            continue

        body = render_template(
            template_id,
            broker_name=broker_name,
            profile=profile,
        )

        email_messages.append(
            EmailMessage(
                to=channel_endpoint,
                subject=f"Data Deletion Request \u2014 {broker_name}",
                body=body,
            )
        )
        endpoint_ids[channel_endpoint].append(req_id)

    if not email_messages:
        return {
            "campaign_id": campaign_id,
            "total_planned": len(requests),
            "batch_size": len(batch),
            "results": [],
        }

    send_results = await send_messages_batch(
        email_messages,
        smtp_config=smtp_config,
    )

    results = []
    # Track how many results we have consumed per endpoint so we can
    # match each result to the correct request when duplicates exist.
    consumed: dict[str, int] = {}
    for sr in send_results:
        to_addr = sr["to"]
        idx = consumed.get(to_addr, 0)
        ids = endpoint_ids.get(to_addr, [])
        req_id = ids[idx] if idx < len(ids) else None
        if req_id is not None:
            consumed[to_addr] = idx + 1

        if sr["success"] and req_id is not None:
            append_event_and_project(
                req_id,
                "SENT",
                payload={
                    "to": to_addr,
                    "account": "smtp",
                    "expected_response_days": 30,
                },
            )
        elif req_id is not None:
            append_event_and_project(
                req_id,
                "SEND_FAILED",
                payload={"error": sr.get("error", ""), "to": to_addr},
            )

        results.append(sr)

    return {
        "campaign_id": campaign_id,
        "total_planned": len(requests),
        "batch_size": len(batch),
        "results": results,
    }


def _select_channel(broker: Broker) -> dict[str, Any] | None:
    from symeraseme.registry.schema import EmailOptOut, WebFormOptOut

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
    from symeraseme.core.db import get_connection

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

    return {"reply_id": reply_id, "request_id": request_id, "classified_as": classified_as}
