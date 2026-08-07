from __future__ import annotations

import asyncio
import json
import logging
from concurrent.futures import ThreadPoolExecutor
from typing import Any, cast

import typer

from symeraseme.adapters.web.playwright_runner import (
    PlaywrightRunnerError,
    WebFormResult,
)
from symeraseme.adapters.web.playwright_runner import (
    run_web_form as _run_form,
)
from symeraseme.core.exceptions import RegistryError
from symeraseme.core.identity import load_profile, profile_exists
from symeraseme.core.manual_fallback import create_manual_task
from symeraseme.core.result_types import CliResult
from symeraseme.registry.loader import load_broker
from symeraseme.registry.schema import WebFormOptOut

logger = logging.getLogger(__name__)


def _build_identity_fields() -> dict[str, str]:
    if not profile_exists():
        return {}
    profile = load_profile()
    name_parts = profile.full_name.split(None, 1)
    first_name = name_parts[0] if name_parts else profile.full_name
    last_name = name_parts[1] if len(name_parts) > 1 else ""

    fields = {
        "full_name": profile.full_name,
        "first_name": first_name,
        "last_name": last_name,
        "email": profile.email_addresses[0] if profile.email_addresses else "",
        "phone_number": profile.phone_numbers[0] if profile.phone_numbers else "",
    }
    for i, addr in enumerate(profile.addresses):
        fields[f"address_street_{i}"] = addr.street
        fields[f"address_city_{i}"] = addr.city
        fields[f"address_zip_{i}"] = addr.postal_code
        fields[f"address_state_{i}"] = addr.state or ""
        fields[f"address_country_{i}"] = addr.country
    return fields


def _warn_missing_state_for_ccpa(
    broker_id: str,
    broker_name: str,
    jurisdictions: list[str],
    identity_fields: dict[str, str],
) -> None:
    if "CCPA" not in jurisdictions:
        return
    for key, value in identity_fields.items():
        if key.startswith("address_state_") and not value:
            logger.warning(
                "Broker '%s' requires CCPA form — address state is missing. "
                "The form may be incomplete or rejected.",
                broker_name,
            )
            return


async def _run_form_with_fallback(
    broker_id: str,
    broker_name: str,
    url: str,
    steps_data: list[dict[str, Any]],
    form: WebFormOptOut,
    *,
    headed: bool = False,
    screenshot_dir: str = "",
    identity_fields: dict[str, str],
) -> tuple[WebFormResult, None, int | None] | tuple[None, str, None]:
    try:
        result = await _run_form(
            url=url,
            steps=steps_data,
            headless=not headed,
            timeout_seconds=form.form_spec.timeout_seconds,
            rate_limit_delay=form.form_spec.rate_limit_delay,
            screenshot_dir=screenshot_dir or None,
            identity_fields=identity_fields,
        )
    except PlaywrightRunnerError as e:
        return None, str(e), None

    task_id = None
    if not result.success:
        task = create_manual_task(
            broker_id=broker_id,
            broker_name=broker_name,
            form_url=url,
            reason="generic_error",
            screenshot_path=result.screenshot_path or "",
            step_index=result.step_index,
            total_steps=result.total_steps,
            error_message=result.error,
        )
        task_id = task.id

    return result, None, task_id


def run_web_form_sync(
    broker_id: str,
    *,
    headed: bool = False,
    screenshot_dir: str = "",
    dry_run: bool = False,
) -> dict[str, Any]:
    """Run a broker's web form, safe with or without a running event loop.

    ``run_web_form_for_broker`` is a coroutine. ``plan execute`` drives the
    SMTP async backend with ``asyncio.run`` inside ``handle_execute``, so this
    runner is invoked while an event loop is already running — a nested
    ``asyncio.run()`` would raise ``RuntimeError``. When no loop is running
    (Himalaya backend, concurrent workers, standalone callers) the coroutine
    is driven directly; otherwise it runs on a fresh loop in a worker thread.
    """
    coroutine = run_web_form_for_broker(
        broker_id, headed=headed, screenshot_dir=screenshot_dir, dry_run=dry_run
    )
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        # No event loop is running in this thread — drive the coroutine directly.
        return asyncio.run(coroutine)
    # Already inside a running event loop: run the coroutine on its own loop
    # in a worker thread and block for the result.
    with ThreadPoolExecutor(max_workers=1, thread_name_prefix="web-form") as pool:
        return pool.submit(asyncio.run, coroutine).result()


async def run_web_form_for_broker(
    broker_id: str,
    headed: bool = False,
    screenshot_dir: str = "",
    dry_run: bool = False,
) -> dict[str, Any]:
    broker = load_broker(broker_id)
    web_forms = [c for c in broker.opt_out if c.type == "web_form"]
    if not web_forms:
        return {
            "success": False,
            "error": f"Broker '{broker_id}' has no web form opt-out channel.",
        }

    form = cast(WebFormOptOut, web_forms[0])
    url = form.url
    steps_data = [s.model_dump(exclude_none=True) for s in form.form_spec.steps]
    identity_fields = _build_identity_fields()
    _warn_missing_state_for_ccpa(broker_id, broker.name, broker.jurisdictions, identity_fields)

    if dry_run:
        body = json.dumps(
            {
                "url": url,
                "steps": steps_data,
                "identity_fields": identity_fields,
            },
            indent=2,
        )
        return {
            "success": True,
            "dry_run": True,
            "broker_id": broker_id,
            "broker_name": broker.name,
            "url": url,
            "steps": steps_data,
            "identity_fields": identity_fields,
            "body": body,
        }

    result, error, task_id = await _run_form_with_fallback(
        broker_id,
        broker.name,
        url,
        steps_data,
        form,
        headed=headed,
        screenshot_dir=screenshot_dir,
        identity_fields=identity_fields,
    )

    if error:
        return {
            "success": False,
            "error": error,
            "broker_id": broker_id,
            "broker_name": broker.name,
        }

    assert result is not None

    return {
        "success": result.success,
        "broker_id": broker_id,
        "broker_name": broker.name,
        "step_index": result.step_index,
        "total_steps": result.total_steps,
        "error": result.error,
        "screenshot_path": result.screenshot_path,
        "task_id": task_id,
    }


async def handle_run_web_form(
    broker_id: str,
    headed: bool = False,
    screenshot_dir: str = "",
    dry_run: bool = False,
) -> CliResult:
    try:
        broker = load_broker(broker_id)
    except (RegistryError, FileNotFoundError, ValueError, RuntimeError, OSError) as e:
        return CliResult(
            success=False,
            error=(
                f"Broker '{broker_id}' not found: {e}. "
                "Run 'symeraseme brokers list' to see available brokers."
            ),
        )

    web_forms = [c for c in broker.opt_out if c.type == "web_form"]
    if not web_forms:
        return CliResult(
            success=False,
            error=(
                f"Broker '{broker_id}' has no web form opt-out channel. "
                "Check 'symeraseme brokers show {broker_id}' for available channels (email, etc.)."
            ),
        )

    form = cast(WebFormOptOut, web_forms[0])
    url = form.url
    steps_data = [s.model_dump(exclude_none=True) for s in form.form_spec.steps]
    identity_fields = _build_identity_fields()
    _warn_missing_state_for_ccpa(broker_id, broker.name, broker.jurisdictions, identity_fields)

    if dry_run:
        lines = [f"[DRY RUN] Would run web form for {broker.name} ({url})"]
        lines.append(f"Steps: {len(steps_data)}")
        for i, step in enumerate(steps_data, 1):
            action = step.get("action", "unknown")
            selector = step.get("selector", "")
            lines.append(f"  Step {i}: {action} {selector}")
        if identity_fields:
            lines.append("Identity fields:")
            for k, v in identity_fields.items():
                lines.append(f"  {k}: {v}")
        return CliResult(
            success=True,
            data={
                "broker_id": broker_id,
                "broker_name": broker.name,
                "dry_run": True,
                "message": "\n".join(lines),
            },
        )

    typer.echo(f"Running web form for {broker.name} ({url})")
    typer.echo(f"Steps: {len(steps_data)}")

    result, error, task_id = await _run_form_with_fallback(
        broker_id,
        broker.name,
        url,
        steps_data,
        form,
        headed=headed,
        screenshot_dir=screenshot_dir,
        identity_fields=identity_fields,
    )

    if error:
        return CliResult(
            success=False,
            error=(
                f"Playwright error: {error}. "
                "Install with: uv pip install playwright && playwright install chromium"
            ),
        )

    assert result is not None

    data = {
        "broker_id": broker_id,
        "success": result.success,
        "step_index": result.step_index,
        "total_steps": result.total_steps,
        "error": result.error,
        "screenshot_path": result.screenshot_path,
        "task_id": task_id,
    }

    if result.success:
        data["message"] = f"Web form completed successfully ({result.total_steps} steps)."
        return CliResult(success=True, data=data)

    msg = (
        f"Web form failed at step {result.step_index + 1}/{result.total_steps}: "
        f"{result.error}. Manual task created. "
        "Run 'symeraseme manual-tasks list' to see it."
    )
    if result.screenshot_path:
        msg += f"\nScreenshot saved to: {result.screenshot_path}"
    return CliResult(success=False, data=data, error=msg)
