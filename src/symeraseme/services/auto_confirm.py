from __future__ import annotations

import asyncio

import typer

from symeraseme.adapters.web.confirmation_clicker import auto_confirm
from symeraseme.core.db_connection import get_connection, with_db
from symeraseme.core.events import get_events, get_removal_request
from symeraseme.core.projection import append_event_and_project
from symeraseme.core.result_types import CliResult


@with_db
def handle_auto_confirm(
    request_id: int,
    headed: bool = False,
    screenshot_dir: str = "",
    dry_run: bool = False,
) -> CliResult:
    req = get_removal_request(request_id)
    if req is None:
        return CliResult(
            success=False,
            error=(
                f"Request #{request_id} not found. "
                "Run 'symeraseme requests list' to see available requests."
            ),
        )

    events = get_events(request_id)
    if not events:
        return CliResult(
            success=False,
            error=(
                f"No events found for request #{request_id}. "
                "Events are created when a request is planned or sent."
            ),
        )

    last_event = events[-1]
    payload = last_event.get("payload_json", {}) or {}
    reply_body = payload.get("snippet", "") or payload.get("template", "") or ""

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    if reply:
        reply_body = reply["snippet"] or reply_body
        from_addr = reply["from_addr"] or ""
    else:
        from_addr = ""

    typer.echo(f"Scanning for confirmation links in reply for request #{request_id}...")

    result = asyncio.run(
        auto_confirm(
            request_id,
            reply_body,
            from_addr=from_addr,
            headless=not headed,
            screenshot_dir=screenshot_dir or None,
            dry_run=dry_run,
        )
    )

    if not dry_run and result.success:
        append_event_and_project(
            request_id,
            "CONFIRMATION_LINK_CLICKED",
            payload={
                "url": result.clicked_url,
                "step": result.step,
                "screenshot_before": result.screenshot_before,
                "screenshot_after": result.screenshot_after,
            },
            source="system",
        )
    elif not dry_run and result.error:
        append_event_and_project(
            request_id,
            "NOTE_ADDED",
            payload={
                "note": f"Auto-confirm failed: {result.error}",
                "url": result.clicked_url,
            },
            source="system",
        )

    data = {
        "request_id": request_id,
        "success": result.success,
        "step": result.step,
        "clicked_url": result.clicked_url,
        "error": result.error,
        "dry_run": result.dry_run,
        "screenshot_before": result.screenshot_before,
        "screenshot_after": result.screenshot_after,
    }

    if result.dry_run:
        data["message"] = f"[DRY RUN] Would click: {result.clicked_url}"
        return CliResult(success=True, data=data)

    if result.success:
        lines = [f"Confirmation link clicked: {result.clicked_url}"]
        lines.append(f"  Step: {result.step}")
        if result.screenshot_before:
            lines.append(f"  Screenshot before: {result.screenshot_before}")
        if result.screenshot_after:
            lines.append(f"  Screenshot after: {result.screenshot_after}")
        data["message"] = "\n".join(lines)
        return CliResult(success=True, data=data)

    msg = (
        f"Auto-confirm failed: {result.error}. "
        "Check the broker reply manually or retry with --headed to see the browser."
    )
    if result.clicked_url:
        msg += f"\n  URL: {result.clicked_url}"
    return CliResult(success=False, data=data, error=msg)
