"""Web-form Automation commands."""

from __future__ import annotations

import asyncio

import typer

from symeraseme.cli.console import render_result
from symeraseme.services.auto_confirm import handle_auto_confirm
from symeraseme.services.captcha import handle_solve_captcha
from symeraseme.services.manual_task import (
    handle_manual_tasks_cleanup,
    handle_manual_tasks_complete,
    handle_manual_tasks_list,
    handle_manual_tasks_show,
)
from symeraseme.services.web_form import handle_run_web_form

manual_tasks_app = typer.Typer(
    name="manual-tasks",
    help="List and manage manual fallback tasks for web forms",
    no_args_is_help=True,
)


def run_web_form(
    ctx: typer.Context,
    broker_id: str = typer.Argument(
        ...,
        help="Broker ID from registry",
    ),
    headed: bool = typer.Option(
        False,
        "--headed",
        help="Show browser window",
    ),
    screenshot_dir: str = typer.Option(
        "",
        "--screenshots",
        help="Directory for screenshots",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show form steps without executing",
    ),
) -> None:
    from symeraseme.cli.console import show_spinner

    with show_spinner("Running web form..."):
        result = asyncio.run(
            handle_run_web_form(
                broker_id,
                headed,
                screenshot_dir,
                dry_run,
                ctx.obj["output"],
            )
        )
    render_result(ctx.obj["output"], result)


def auto_confirm_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to auto-confirm",
    ),
    headed: bool = typer.Option(
        False,
        "--headed",
        help="Show browser window",
    ),
    screenshot_dir: str = typer.Option(
        "",
        "--screenshots",
        help="Directory for screenshots",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Simulate without clicking",
    ),
) -> None:
    result = handle_auto_confirm(
        request_id,
        headed,
        screenshot_dir,
        dry_run,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


def solve_captcha_cmd(
    ctx: typer.Context,
    provider: str = typer.Option(
        "capsolver",
        "--provider",
        help="Captcha provider: capsolver or twocaptcha",
    ),
    api_key: str = typer.Option(
        None,
        "--api-key",
        envvar="CAPSOLVER_API_KEY",
        help="API key (or set CAPSOLVER_API_KEY)",
    ),
    site_key: str = typer.Option(
        ...,
        "--site-key",
        prompt=True,
        help="reCAPTCHA site key",
    ),
    page_url: str = typer.Option(
        ...,
        "--page-url",
        prompt=True,
        help="Page URL where captcha appears",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show captcha parameters without solving",
    ),
) -> None:
    result = handle_solve_captcha(
        provider,
        api_key,
        site_key,
        page_url,
        dry_run,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@manual_tasks_app.command(name="list")
def manual_tasks_list(
    ctx: typer.Context,
    status: str = typer.Option(
        None,
        "--status",
        help="Filter by status (pending, completed, cancelled)",
    ),
    request_id: int = typer.Option(
        None,
        "--request-id",
        help="Filter by request ID",
    ),
) -> None:
    result = handle_manual_tasks_list(
        status,
        request_id,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@manual_tasks_app.command(name="show")
def manual_tasks_show(
    ctx: typer.Context,
    task_id: int = typer.Argument(..., help="Task ID to show"),
) -> None:
    result = handle_manual_tasks_show(task_id, ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@manual_tasks_app.command(name="complete")
def manual_tasks_complete(
    ctx: typer.Context,
    task_id: int = typer.Argument(
        ...,
        help="Task ID to mark as completed",
    ),
    notes: str = typer.Option(
        "",
        "--notes",
        help="Optional completion notes",
    ),
) -> None:
    result = handle_manual_tasks_complete(
        task_id,
        notes,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@manual_tasks_app.command(name="cleanup")
def manual_tasks_cleanup(
    ctx: typer.Context,
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show what would be removed without deleting",
    ),
) -> None:
    result = handle_manual_tasks_cleanup(
        dry_run,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)
