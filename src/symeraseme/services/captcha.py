from __future__ import annotations

import typer

from symeraseme.adapters.web.captcha_solver import CaptchaError, create_solver
from symeraseme.cli.console import render_error
from symeraseme.core.result_types import CliResult


def handle_solve_captcha(
    provider: str = "capsolver",
    api_key: str | None = None,
    site_key: str | None = None,
    page_url: str | None = None,
    dry_run: bool = False,
) -> CliResult:
    if dry_run:
        return CliResult(
            success=True,
            data={
                "provider": provider,
                "site_key": site_key,
                "page_url": page_url,
                "dry_run": True,
                "message": f"[DRY RUN] Would solve captcha via {provider}:\n  site_key: {site_key}\n  page_url: {page_url}",
            },
        )

    typer.echo(f"Solving captcha via {provider}...")

    if site_key is None or page_url is None:
        render_error("site_key and page_url are required")

    try:
        solver = create_solver(provider, api_key=api_key)
        result = solver.solve_recaptcha_v2(
            site_key=site_key,
            page_url=page_url,
        )
    except CaptchaError as e:
        render_error(
            f"Captcha solving failed: {e}. "
            "Check your API key, site_key, and page_url. "
            "Set CAPSOLVER_API_KEY or TWOCAPTCHA_API_KEY env var."
        )

    return CliResult(
        success=True,
        data={
            "provider": provider,
            "task_id": result.task_id,
            "token": result.token,
            "message": f"Captcha solved (task: {result.task_id})\nToken: {result.token[:50]}...",
        },
    )
