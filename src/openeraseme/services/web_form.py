from __future__ import annotations

import asyncio
import json
from typing import cast

import typer

from openeraseme.adapters.web.playwright_runner import (
    PlaywrightRunnerError,
)
from openeraseme.adapters.web.playwright_runner import (
    run_web_form as _run_form,
)
from openeraseme.core.identity import load_profile, profile_exists
from openeraseme.registry.loader import load_broker
from openeraseme.registry.schema import WebFormOptOut


def handle_run_web_form(
    broker_id: str,
    headed: bool = False,
    screenshot_dir: str = "",
    output_format: str = "text",
) -> str:
    try:
        broker = load_broker(broker_id)
    except Exception as e:
        typer.echo(
            f"Broker '{broker_id}' not found: {e}. "
            "Run 'openeraseme brokers list' to see available brokers.",
            err=True,
        )
        raise typer.Exit(1) from e

    web_forms = [c for c in broker.opt_out if c.type == "web_form"]
    if not web_forms:
        typer.echo(
            f"Broker '{broker_id}' has no web form opt-out channel. "
            "Check 'openeraseme brokers show {broker_id}' for available channels (email, etc.).",
            err=True,
        )
        raise typer.Exit(1)

    form = cast(WebFormOptOut, web_forms[0])
    url = form.url
    steps_data = [s.model_dump(exclude_none=True) for s in form.form_spec.steps]

    identity_fields: dict[str, str] = {}
    if profile_exists():
        profile = load_profile()
        name_parts = profile.full_name.split(None, 1)
        first_name = name_parts[0] if name_parts else profile.full_name
        last_name = name_parts[1] if len(name_parts) > 1 else ""

        identity_fields = {
            "full_name": profile.full_name,
            "first_name": first_name,
            "last_name": last_name,
            "email": profile.email_addresses[0] if profile.email_addresses else "",
            "phone_number": profile.phone_numbers[0] if profile.phone_numbers else "",
        }
        for i, addr in enumerate(profile.addresses):
            identity_fields[f"address_street_{i}"] = addr.street
            identity_fields[f"address_city_{i}"] = addr.city
            identity_fields[f"address_zip_{i}"] = addr.postal_code
            identity_fields[f"address_state_{i}"] = addr.state if hasattr(addr, "state") else ""
            identity_fields[f"address_country_{i}"] = addr.country

    typer.echo(f"Running web form for {broker.name} ({url})")
    typer.echo(f"Steps: {len(steps_data)}")

    try:
        result = asyncio.run(
            _run_form(
                url=url,
                steps=steps_data,
                headless=not headed,
                timeout_seconds=form.form_spec.timeout_seconds,
                rate_limit_delay=form.form_spec.rate_limit_delay,
                screenshot_dir=screenshot_dir or None,
                identity_fields=identity_fields,
            )
        )
    except PlaywrightRunnerError as e:
        typer.echo(
            f"Playwright error: {e}. "
            "Install with: uv pip install playwright && playwright install chromium",
            err=True,
        )
        raise typer.Exit(1) from e

    if output_format == "json":
        return json.dumps(
            {
                "broker_id": broker_id,
                "success": result.success,
                "step_index": result.step_index,
                "total_steps": result.total_steps,
                "error": result.error,
                "screenshot_path": result.screenshot_path,
            },
            indent=2,
        )

    if result.success:
        return f"Web form completed successfully ({result.total_steps} steps)."

    typer.echo(
        f"Web form failed at step {result.step_index + 1}/{result.total_steps}: {result.error}. "
        "A manual task has been created. Run 'openeraseme manual-tasks list' to see it.",
    )
    if result.screenshot_path:
        typer.echo(f"Screenshot saved to: {result.screenshot_path}")
    raise typer.Exit(1)
