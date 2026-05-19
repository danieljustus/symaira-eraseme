"""Campaign lifecycle CLI handlers (plan, execute)."""

from __future__ import annotations

import json

from openeraseme.core.consent import check_consent
from openeraseme.core.db import init_db
from openeraseme.core.orchestrator import execute_campaign, get_plan, plan_campaign


def handle_plan_create(
    campaign_id: str,
    jurisdiction: str | None = None,
    priority: str | None = None,
    max_brokers: int = 30,
    output_format: str = "text",
) -> str:
    init_db()
    result = plan_campaign(
        campaign_id=campaign_id,
        jurisdiction=jurisdiction,
        priority=priority,
        max_brokers=max_brokers,
    )
    if output_format == "json":
        return json.dumps(result, indent=2, default=str)

    lines = [f"Campaign: {result['campaign_id']}"]
    lines.append(f"  Total brokers scanned: {result['total_brokers']}")
    lines.append(f"  Planned requests: {result['planned']}")
    for r in result["requests"]:
        lines.append(f"    #{r['request_id']} {r['broker_name']} ({r['channel']})")
    return "\n".join(lines)


def handle_plan_show(
    campaign_id: str | None = None,
    status: str | None = None,
    output_format: str = "text",
) -> str:
    init_db()
    result = get_plan(campaign_id=campaign_id, status=status)

    if output_format == "json":
        return json.dumps(result, indent=2, default=str)

    lines = [f"Plan: {result['campaign_id']} ({result['total']} requests)"]
    for r in result["requests"]:
        status_str = r.get("current_status", "PLANNED")
        lines.append(f"  #{r['id']} [{status_str}] {r.get('broker_id', '?')}")
    return "\n".join(lines)


def handle_execute(
    campaign_id: str,
    account: str | None = None,
    batch_size: int = 5,
    dry_run: bool = False,
    yes: bool = False,
    consent_token: str | None = None,
    output_format: str = "text",
) -> str:
    if not dry_run and not check_consent("execute", yes=yes, consent_token=consent_token):
        import typer

        typer.echo(
            "Error: Destructive command requires consent. "
            "Use --yes or issue a token via 'grant' command.",
            err=True,
        )
        raise typer.Exit(1)

    init_db()
    result = execute_campaign(
        campaign_id,
        account=account,
        batch_size=batch_size,
        dry_run=dry_run,
    )

    if output_format == "json":
        return json.dumps(result, indent=2, default=str)

    lines = []
    for r in result["results"]:
        status = "OK" if r["success"] else "FAIL"
        extra = r.get("dry_run", False) and " (dry-run)" or ""
        lines.append(f"  #{r['request_id']} {status}{extra}")
        if not r["success"]:
            lines.append(f"    Error: {r.get('error', 'unknown')}")
    return "\n".join(lines)
