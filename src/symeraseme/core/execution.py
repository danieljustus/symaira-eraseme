"""Single-request execution: web-form and email dispatch."""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from symeraseme.core.protocols import EmailSender, WebFormRunner
from symeraseme.core.events import get_events, get_removal_request
from symeraseme.core.exceptions import (
    ExecutionError,
    ProfileError,
    RequestNotFoundError,
    SymerasemeError,
)
from symeraseme.core.identity import hash_profile, load_profile
from symeraseme.core.projection import append_event_and_project
from symeraseme.core.templating import render_template

logger = logging.getLogger(__name__)


def _execute_webform_request(
    request_id: int,
    broker_name: str,
    web_form_runner: WebFormRunner | None,
    dry_run: bool,
) -> dict[str, Any]:
    """Execute a web-form based removal request."""
    if web_form_runner is None:
        if dry_run:
            return {
                "success": True,
                "request_id": request_id,
                "dry_run": True,
                "url": "",
                "body": f"[dry-run web form for {broker_name}]",
            }
        msg = (
            "web_form_runner is required for web_form requests. "
            "Pass a concrete WebFormRunner to execute_request()."
        )
        raise ValueError(msg)

    result = web_form_runner(broker_name, dry_run=dry_run)
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


def _execute_email_request(
    request_id: int,
    broker_name: str,
    *,
    payload: dict[str, Any],
    account: str | None = None,
    config_path: str | None = None,
    dry_run: bool = False,
    template_id: str = "",
    email_sender: EmailSender | None = None,
) -> dict[str, Any]:
    """Execute an email-based removal request."""
    channel_endpoint = payload.get("endpoint", "")
    required_fields = payload.get("required_fields", ["full_name", "email_addresses"])

    try:
        profile = load_profile()
        identity_hash = hash_profile(profile)
    except FileNotFoundError as e:
        raise ProfileError(
            "Identity profile not found. "
            "Run 'symeraseme init-profile' first to create your identity profile."
        ) from e

    missing = []
    profile_vars = profile.model_dump()
    for field in required_fields:
        value = profile_vars.get(field)
        if value is None or value == [] or value == {} or value == "":
            missing.append(field)
    if missing:
        raise ProfileError(
            f"Missing required identity fields: {', '.join(missing)}. "
            "Run 'symeraseme init-profile' to update your profile."
        )

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

    if email_sender is None:
        msg = (
            "email_sender is required for email-based requests. "
            "Pass a concrete EmailSender to execute_request()."
        )
        raise ValueError(msg)

    try:
        rendered = render_template(
            template_id,
            profile=profile,
            broker_name=broker_name,
        )
        send_result = email_sender(
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
    except (SymerasemeError, OSError) as e:
        logger.warning("Send failed for %s: %s", request_id, e)
        append_event_and_project(
            request_id,
            "SEND_FAILED",
            payload={"error": str(e), "to": channel_endpoint},
        )
        raise ExecutionError(str(e), request_id=request_id) from e


def execute_request(
    request_id: int,
    *,
    account: str | None = None,
    config_path: str | None = None,
    dry_run: bool = False,
    web_form_runner: WebFormRunner | None = None,
    email_sender: EmailSender | None = None,
) -> dict[str, Any]:
    """Execute a single removal request by sending an email or running a web form."""
    req = get_removal_request(request_id)
    if req is None:
        raise RequestNotFoundError(request_id)

    broker_name = req["broker_id"]
    events = get_events(request_id)
    last_event = events[-1] if events else {}
    payload = last_event.get("payload_json", {})

    channel_type = req.get("channel", "email")

    if channel_type == "web_form":
        return _execute_webform_request(
            request_id,
            broker_name,
            web_form_runner=web_form_runner,
            dry_run=dry_run,
        )

    return _execute_email_request(
        request_id,
        broker_name,
        payload=payload,
        account=account,
        config_path=config_path,
        dry_run=dry_run,
        template_id=req.get("template_id", ""),
        email_sender=email_sender,
    )
