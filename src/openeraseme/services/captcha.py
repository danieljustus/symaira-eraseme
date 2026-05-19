from __future__ import annotations

import json

import typer

from openeraseme.adapters.web.captcha_solver import CaptchaError, create_solver


def handle_solve_captcha(
    provider: str = "capsolver",
    api_key: str | None = None,
    site_key: str | None = None,
    page_url: str | None = None,
    output_format: str = "text",
) -> str:
    typer.echo(f"Solving captcha via {provider}...")

    if site_key is None or page_url is None:
        typer.echo("site_key and page_url are required", err=True)
        raise typer.Exit(1)

    try:
        solver = create_solver(provider, api_key=api_key)
        result = solver.solve_recaptcha_v2(
            site_key=site_key,
            page_url=page_url,
        )
    except CaptchaError as e:
        typer.echo(f"Captcha solving failed: {e}", err=True)
        raise typer.Exit(1) from e

    if output_format == "json":
        return json.dumps(
            {
                "provider": provider,
                "task_id": result.task_id,
                "token": result.token,
            },
            indent=2,
        )

    return f"Captcha solved (task: {result.task_id})\nToken: {result.token[:50]}..."
