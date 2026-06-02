"""Campaign lifecycle CLI handlers (plan, execute)."""

from __future__ import annotations

import asyncio

from symeraseme.core.consent import check_consent
from symeraseme.core.db import init_db
from symeraseme.core.orchestrator import (
    execute_campaign,
    execute_campaign_async,
    get_plan,
    plan_campaign,
)
from symeraseme.core.result_types import CliResult


def handle_plan_create(
    campaign_id: str,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    max_brokers: int = 30,
) -> CliResult:
    init_db()
    """plan create."""
    result = plan_campaign(
        campaign_id=campaign_id,
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        max_brokers=max_brokers,
    )

    lines = [f"Campaign: {result['campaign_id']}"]
    lines.append(f"  Total brokers scanned: {result['total_brokers']}")
    lines.append(f"  Planned requests: {result['planned']}")
    for r in result["requests"]:
        lines.append(f"    #{r['request_id']} {r['broker_name']} ({r['channel']})")

    result["message"] = "\n".join(lines)
    return CliResult(success=True, data=result)


def handle_plan_show(
    campaign_id: str | None = None,
    status: str | None = None,
) -> CliResult:
    init_db()
    """plan show."""
    result = get_plan(campaign_id=campaign_id, status=status)

    lines = [f"Plan: {result['campaign_id']} ({result['total']} requests)"]
    for r in result["requests"]:
        status_str = r.get("current_status", "PLANNED")
        lines.append(f"  #{r['id']} [{status_str}] {r.get('broker_id', '?')}")

    result["message"] = "\n".join(lines)
    return CliResult(success=True, data=result)


def handle_execute(
    campaign_id: str,
    account: str | None = None,
    batch_size: int = 5,
    dry_run: bool = False,
    yes: bool = False,
    consent_token: str | None = None,
    consent_file: str | None = None,
    web_form_runner=None,
    backend: str | None = None,
) -> CliResult:
    if not dry_run and not check_consent(
        "execute",
        yes=yes,
        consent_token=consent_token,
        consent_file=consent_file,
    ):
        from symeraseme.cli.console import render_error

        render_error(
            "Destructive command requires consent. Use --yes or issue a token via 'grant' command."
        )

    init_db()

    if backend is None:
        backend = "himalaya" if account else "smtp"

    import logging

    logger = logging.getLogger(__name__)
    logger.info("Using %s backend for campaign execution", backend)

    if backend == "himalaya":
        result = execute_campaign(
            campaign_id,
            account=account or "",
            batch_size=batch_size,
            dry_run=dry_run,
            web_form_runner=web_form_runner,
        )
    else:
        result = asyncio.run(
            execute_campaign_async(
                campaign_id,
                batch_size=batch_size,
                dry_run=dry_run,
                web_form_runner=web_form_runner,
            )
        )

    lines = []
    any_failure = False
    for r in result["results"]:
        status = "OK" if r["success"] else "FAIL"
        if not r["success"]:
            any_failure = True
        extra = r.get("dry_run", False) and " (dry-run)" or ""
        lines.append(f"  #{r['request_id']} {status}{extra}")
        if not r["success"]:
            lines.append(f"    Error: {r.get('error', 'unknown')}")

    result["message"] = "\n".join(lines)
    return CliResult(success=not any_failure, data=result)
