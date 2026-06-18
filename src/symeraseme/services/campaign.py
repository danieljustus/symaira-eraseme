"""Campaign lifecycle CLI handlers (plan, execute)."""

from __future__ import annotations

import asyncio
import logging

from symeraseme.core.batch import execute_campaign, execute_campaign_async
from symeraseme.core.consent import check_consent
from symeraseme.core.db_connection import init_db
from symeraseme.core.exceptions import safe_error_str
from symeraseme.core.planning import get_plan, plan_campaign
from symeraseme.core.result_types import CliResult

logger = logging.getLogger(__name__)


def handle_plan_create(
    campaign_id: str,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    max_brokers: int = 30,
) -> CliResult:
    init_db()
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
    email_sender=None,
    concurrent: bool = False,
    workers: int = 3,
) -> CliResult:
    if not dry_run and not check_consent(
        "execute",
        yes=yes,
        consent_token=consent_token,
        consent_file=consent_file,
    ):
        return CliResult(
            success=False,
            error=(
                "Destructive command requires consent. "
                "Use --yes or issue a token via 'grant' command."
            ),
        )

    init_db()

    if backend is None:
        backend = "himalaya" if account else "smtp"

    if backend == "himalaya":
        if not account:
            msg = (
                "Himalaya backend requires --account. "
                "Use --account <name> or switch to SMTP with --backend smtp."
            )
            return CliResult(success=False, error=msg)
        if not dry_run:
            from symeraseme.adapters.email.himalaya import himalaya_available

            if not himalaya_available():
                msg = (
                    "Himalaya CLI is not installed. "
                    "Install it via 'cargo install himalaya' "
                    "or 'brew install himalaya', or use --backend smtp."
                )
                return CliResult(success=False, error=msg)
        logger.info("Using Himalaya backend (account=%s)", account)
    elif backend == "smtp":
        if not dry_run:
            from symeraseme.adapters.email.himalaya import load_smtp_config

            smtp_config = load_smtp_config()
            if not smtp_config.from_addr:
                msg = (
                    "SMTP backend requires SYMERASEME_SMTP_FROM "
                    "to be set. Configure it in your environment or .env file."
                )
                return CliResult(success=False, error=msg)
            logger.debug("Using SMTP backend (host=%s:%s)", smtp_config.host, smtp_config.port)
        else:
            logger.info("Using SMTP backend (dry-run)")
    else:
        return CliResult(
            success=False,
            error=f"Unknown backend '{backend}'. Use 'smtp' or 'himalaya'.",
        )

    if email_sender is None and backend == "himalaya":
        from symeraseme.adapters.email.himalaya import send_email

        email_sender = send_email

    if backend == "himalaya":
        result = execute_campaign(
            campaign_id,
            account=account or "",
            batch_size=batch_size,
            dry_run=dry_run,
            web_form_runner=web_form_runner,
            email_sender=email_sender,
        )
    elif concurrent:
        logger.info("Using concurrent execution with %d workers", workers)

        async def _run_concurrent() -> dict:
            import asyncio

            semaphore = asyncio.Semaphore(workers)

            async def _limited_execute(req: dict) -> dict:
                async with semaphore:
                    from symeraseme.core.execution import execute_request

                    try:
                        return await asyncio.to_thread(
                            execute_request,
                            req["id"],
                            dry_run=dry_run,
                            web_form_runner=web_form_runner,
                            email_sender=email_sender,
                        )
                    except Exception as e:
                        return {"success": False, "error": safe_error_str(e), "request_id": req["id"]}

            from symeraseme.core.batch import _prepare_batch

            batch = _prepare_batch(campaign_id, batch_size)
            if not batch:
                return {
                    "campaign_id": campaign_id,
                    "total_planned": 0,
                    "batch_size": 0,
                    "results": [],
                }

            tasks = [_limited_execute(req) for req in batch]
            results = await asyncio.gather(*tasks)
            return {
                "campaign_id": campaign_id,
                "total_planned": len(batch),
                "batch_size": len(batch),
                "results": list(results),
            }

        result = asyncio.run(_run_concurrent())
    else:
        result = asyncio.run(
            execute_campaign_async(
                campaign_id,
                batch_size=batch_size,
                dry_run=dry_run,
                web_form_runner=web_form_runner,
                email_sender=email_sender,
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
