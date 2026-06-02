"""Batch campaign execution with SMTP and progress tracking."""

from __future__ import annotations

from collections import defaultdict
from typing import Any

from rich.console import Console
from rich.progress import BarColumn, Progress, SpinnerColumn, TextColumn

from symeraseme.adapters.email.himalaya import (
    EmailMessage,
    SmtpConfig,
    load_smtp_config,
    send_messages_batch,
)
from symeraseme.core.events import get_events_for_requests, list_removal_requests
from symeraseme.core.exceptions import SymerasemeError
from symeraseme.core.execution import execute_request
from symeraseme.core.identity import load_profile
from symeraseme.core.projection import append_event_and_project
from symeraseme.core.templating import render_template

logger = __import__("logging").getLogger(__name__)
_PROGRESS_CONSOLE = Console(stderr=True)
_BATCH_LIMIT = 10


def _prepare_batch(campaign_id: str, batch_size: int) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Fetch planned requests and slice to the requested batch size."""
    requests = list_removal_requests(campaign_id=campaign_id, status="PLANNED")
    return requests, requests[:batch_size]


def _build_result(
    campaign_id: str, requests: list[dict[str, Any]], batch: list[dict[str, Any]], results: list[dict[str, Any]]
) -> dict[str, Any]:
    """Return the standard campaign execution result dict."""
    return {
        "campaign_id": campaign_id,
        "total_planned": len(requests),
        "batch_size": len(batch),
        "results": results,
    }


def execute_campaign(
    campaign_id: str,
    *,
    account: str | None = None,
    config_path: str | None = None,
    batch_size: int = 5,
    dry_run: bool = False,
    web_form_runner=None,
    email_sender=None,
) -> dict[str, Any]:
    requests, batch = _prepare_batch(campaign_id, batch_size)
    results: list[dict[str, Any]] = []
    for req in batch:
        try:
            result = execute_request(
                req["id"],
                account=account,
                config_path=config_path,
                dry_run=dry_run,
                web_form_runner=web_form_runner,
                email_sender=email_sender,
            )
        except SymerasemeError as e:
            result = {"success": False, "error": str(e), "request_id": req["id"]}
        results.append(result)
    return _build_result(campaign_id, requests, batch, results)


def _load_smtp_config(smtp_skip_tls: bool = False) -> Any:
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
    return smtp_config


def _gather_email_messages(
    batch: list[dict[str, Any]],
    events_by_rid: dict[int, list[dict[str, Any]]],
    profile: Any,
    progress: Any,
    task: Any,
) -> tuple[list[Any], defaultdict[str, list[int]]]:
    """Render email messages for a batch of requests.

    Returns (email_messages, endpoint_ids) where endpoint_ids maps
    each recipient address to its associated request IDs (for FIFO
    matching when processing SMTP results).
    """
    email_messages: list[Any] = []
    endpoint_ids: defaultdict[str, list[int]] = defaultdict(list)
    for req in batch:
        req_id = req["id"]
        broker_name = req["broker_id"]
        progress.update(task, description=f"Preparing {broker_name}...")
        events = events_by_rid.get(req_id, [])
        last_event = events[-1] if events else {}
        payload = last_event.get("payload_json", {}) if isinstance(last_event, dict) else {}
        channel_endpoint = payload.get("endpoint", "")
        template_id = req.get("template_id", "")
        if not channel_endpoint:
            progress.advance(task)
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
        progress.advance(task)
    return email_messages, endpoint_ids


def _apply_batch_results(
    send_results: list[dict[str, Any]],
    endpoint_ids: defaultdict[str, list[int]],
    progress: Any,
    task: Any,
) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    consumed: dict[str, int] = {}
    for sr in send_results:
        to_addr = sr["to"]
        progress.update(task, description=f"Recording result for {to_addr}...")
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
    return results


async def execute_campaign_async(
    campaign_id: str,
    *,
    batch_size: int = _BATCH_LIMIT,
    dry_run: bool = False,
    smtp_skip_tls: bool = False,
    web_form_runner=None,
    email_sender=None,
) -> dict[str, Any]:
    requests = list_removal_requests(campaign_id=campaign_id, status="PLANNED")
    batch = requests[:batch_size]
    try:
        profile = load_profile()
    except FileNotFoundError:
        profile = None
    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        TextColumn("[progress.completed]{task.completed}/{task.total}"),
        transient=True,
        console=_PROGRESS_CONSOLE,
    ) as progress:
        task = progress.add_task("Sending removal requests...", total=len(batch))
        if dry_run:
            results: list[dict[str, Any]] = []
            for req in batch:
                progress.update(task, description=f"Processing {req['broker_id']}...")
                try:
                    r = execute_request(
                        req["id"],
                        dry_run=True,
                        web_form_runner=web_form_runner,
                        email_sender=email_sender,
                    )
                except SymerasemeError as e:
                    r = {"success": False, "error": str(e), "request_id": req["id"]}
                results.append(r)
                progress.advance(task)
            return {
                "campaign_id": campaign_id,
                "total_planned": len(requests),
                "batch_size": len(batch),
                "results": results,
            }
        smtp_config = _load_smtp_config(smtp_skip_tls)
        batch_ids = [r["id"] for r in batch]
        events_by_rid = get_events_for_requests(batch_ids) if batch_ids else {}
        email_messages, endpoint_ids = _gather_email_messages(
            batch, events_by_rid, profile, progress, task
        )
        if not email_messages:
            return {
                "campaign_id": campaign_id,
                "total_planned": len(requests),
                "batch_size": len(batch),
                "results": [],
            }
        progress.update(task, description="Sending batch via SMTP...", completed=len(batch))
        send_results = await send_messages_batch(email_messages, smtp_config=smtp_config)
        results = _apply_batch_results(send_results, endpoint_ids, progress, task)
    return {
        "campaign_id": campaign_id,
        "total_planned": len(requests),
        "batch_size": len(batch),
        "results": results,
    }
