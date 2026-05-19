from __future__ import annotations

import os
from enum import StrEnum

import typer

from openeraseme.services.account import (
    handle_account_add,
    handle_account_list,
    handle_account_remove,
)
from openeraseme.services.auto_confirm import handle_auto_confirm
from openeraseme.services.campaign import (
    handle_execute,
    handle_plan_create,
    handle_plan_show,
)
from openeraseme.services.captcha import handle_solve_captcha
from openeraseme.services.consent import handle_grant
from openeraseme.services.db import handle_db_init
from openeraseme.services.inbox import handle_poll_inbox
from openeraseme.services.manual_task import (
    handle_manual_tasks_complete,
    handle_manual_tasks_list,
    handle_manual_tasks_show,
)
from openeraseme.services.profile import (
    handle_init_profile,
    handle_render_template,
    handle_show_profile,
    handle_version,
)
from openeraseme.services.reply import (
    handle_classify_reply,
    handle_generate_rebuttal,
)
from openeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)
from openeraseme.services.request import (
    handle_events_show,
    handle_requests_list,
)
from openeraseme.services.scheduler import (
    handle_generate_scheduler,
    handle_schedule_install,
    handle_schedule_status,
    handle_schedule_uninstall,
)
from openeraseme.services.tick import handle_tick
from openeraseme.services.web_form import handle_run_web_form


class OutputFormat(StrEnum):
    text = "text"
    json = "json"


app = typer.Typer(
    name="openeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
)
accounts_app = typer.Typer(
    name="accounts",
    help="Manage email accounts (OAuth2 setup, list, remove)",
    no_args_is_help=True,
)
app.add_typer(accounts_app)
plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)
app.add_typer(plan_app)
events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
app.add_typer(events_app)
requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
app.add_typer(requests_app)
manual_tasks_app = typer.Typer(
    name="manual-tasks",
    help="List and manage manual fallback tasks for web forms",
    no_args_is_help=True,
)
app.add_typer(manual_tasks_app)
schedule_app = typer.Typer(
    name="schedule",
    help="Manage scheduler configuration (install, uninstall, status)",
    no_args_is_help=True,
)
app.add_typer(schedule_app)


@app.callback()
def main(ctx: typer.Context, output: OutputFormat = OutputFormat.text) -> None:
    ctx.ensure_object(dict)
    ctx.obj["output"] = output


@app.command()
def version() -> None:
    typer.echo(handle_version())


@app.command()
def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    typer.echo(handle_init_profile(full_name, email))


@app.command()
def show_profile() -> None:
    typer.echo(handle_show_profile())


@app.command()
def render_template(
    template: str = typer.Argument(
        help="Template name (e.g. gdpr-art17.de.md.j2)",
    ),
    broker_name: str = typer.Option("", help="Name of the data broker"),
    broker_website: str = typer.Option("", help="Broker website URL"),
) -> None:
    typer.echo(handle_render_template(template, broker_name, broker_website))


@accounts_app.command()
def add(
    provider: str = typer.Argument(help="Provider: gmail or outlook"),
    email: str = typer.Option(..., prompt=True, help="Email address"),
    client_id: str = typer.Option(..., prompt=True, help="OAuth2 client ID"),
    client_secret: str = typer.Option(
        ...,
        prompt=True,
        hide_input=True,
        help="OAuth2 client secret",
    ),
) -> None:
    typer.echo(handle_account_add(provider, email, client_id, client_secret))


@accounts_app.command()
def list_cmd() -> None:
    typer.echo(handle_account_list())


@accounts_app.command()
def remove(email: str = typer.Argument(help="Email address to remove")) -> None:
    typer.echo(handle_account_remove(email))


@plan_app.command()
def create(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ...,
        "--campaign",
        help="Campaign identifier (e.g. initial-2026-Q2)",
    ),
    jurisdiction: str = typer.Option(
        None,
        help="Filter by jurisdiction (e.g. DE, US)",
    ),
    priority: str = typer.Option(
        None,
        help="Filter by priority (high, medium, low)",
    ),
    max_brokers: int = typer.Option(
        30,
        "--max",
        help="Maximum brokers to plan",
    ),
) -> None:
    typer.echo(
        handle_plan_create(
            campaign_id,
            jurisdiction,
            priority,
            max_brokers,
            ctx.obj["output"],
        )
    )


@plan_app.command(name="show")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    typer.echo(handle_plan_show(campaign_id, status, ctx.obj["output"]))


@app.command()
def execute(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ...,
        "--campaign",
        help="Campaign to execute",
    ),
    account: str = typer.Option(
        None,
        "--account",
        help="Himalaya account name",
    ),
    batch_size: int = typer.Option(
        5,
        "--batch-size",
        help="Number to send",
    ),
    dry_run: bool = typer.Option(False, "--dry-run", help="Simulate only"),
    yes: bool = typer.Option(
        False,
        "--yes",
        help="Skip consent prompt (destructive)",
    ),
    consent_token: str = typer.Option(
        None,
        "--consent",
        help="Pre-issued consent token",
    ),
) -> None:
    typer.echo(
        handle_execute(
            campaign_id,
            account,
            batch_size,
            dry_run,
            yes,
            consent_token,
            ctx.obj["output"],
        )
    )


@app.command()
def grant(
    ctx: typer.Context,
    command: str = typer.Argument(
        "execute",
        help="Command to authorize (e.g. execute)",
    ),
    ttl: int = typer.Option(86400, "--ttl", help="Token TTL in seconds"),
    revoke: str = typer.Option(
        None,
        "--revoke",
        help="Revoke a consent token",
    ),
    revoke_all: bool = typer.Option(
        False,
        "--revoke-all",
        help="Revoke all active tokens",
    ),
    list_tokens: bool = typer.Option(
        False,
        "--list",
        help="List active tokens",
    ),
) -> None:
    typer.echo(
        handle_grant(
            command,
            ttl,
            revoke,
            revoke_all,
            list_tokens,
            ctx.obj["output"],
        )
    )


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    typer.echo(handle_events_show(request_id, ctx.obj["output"]))


@requests_app.command(name="list")
def requests_list(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        None,
        "--campaign",
        help="Filter by campaign",
    ),
    status: str = typer.Option(None, "--status", help="Filter by status"),
    broker_id: str = typer.Option(
        None,
        "--broker",
        help="Filter by broker ID",
    ),
) -> None:
    typer.echo(
        handle_requests_list(
            campaign_id,
            status,
            broker_id,
            ctx.obj["output"],
        )
    )


@app.command(name="poll-inbox")
def poll_inbox(
    ctx: typer.Context,
    host: str = typer.Option("imap.gmail.com", "--host", help="IMAP server"),
    port: int = typer.Option(993, "--port", help="IMAP port"),
    username: str = typer.Option(
        ...,
        "--username",
        prompt=True,
        help="IMAP username",
    ),
    since_days: int = typer.Option(1, "--since", help="Look back N days"),
    ssl: bool = typer.Option(True, "--ssl/--no-ssl"),
    campaign_id: str = typer.Option(
        None,
        "--campaign",
        help="Campaign to match replies against",
    ),
) -> None:
    password = os.environ.get("IMAP_PASSWORD") or typer.prompt(
        "IMAP password",
        hide_input=True,
    )
    typer.echo(
        handle_poll_inbox(
            host,
            port,
            username,
            since_days,
            ssl,
            campaign_id,
            password,
            ctx.obj["output"],
        )
    )


@app.command()
def tick(
    ctx: typer.Context,
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show actions without executing",
    ),
) -> None:
    typer.echo(handle_tick(dry_run, ctx.obj["output"]))


@app.command(name="classify-reply")
def classify_reply(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to classify the reply for",
    ),
    api_key: str = typer.Option(
        None,
        "--api-key",
        envvar="ANTHROPIC_API_KEY",
        help="Anthropic API key",
    ),
    model: str = typer.Option(
        "claude-3-5-sonnet-latest",
        "--model",
        help="Claude model name",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save classification result to DB",
    ),
) -> None:
    typer.echo(
        handle_classify_reply(
            request_id,
            api_key,
            model,
            save,
            ctx.obj["output"],
        )
    )


@app.command(name="run-web-form")
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
) -> None:
    typer.echo(
        handle_run_web_form(
            broker_id,
            headed,
            screenshot_dir,
            ctx.obj["output"],
        )
    )


@app.command(name="auto-confirm")
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
    typer.echo(
        handle_auto_confirm(
            request_id,
            headed,
            screenshot_dir,
            dry_run,
            ctx.obj["output"],
        )
    )


@app.command(name="generate-rebuttal")
def generate_rebuttal_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to generate rebuttal for",
    ),
    api_key: str = typer.Option(
        None,
        "--api-key",
        envvar="ANTHROPIC_API_KEY",
        help="Anthropic API key",
    ),
    model: str = typer.Option(
        "claude-3-5-sonnet-latest",
        "--model",
        help="Claude model name",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save rebuttal to DB",
    ),
) -> None:
    typer.echo(
        handle_generate_rebuttal(
            request_id,
            api_key,
            model,
            save,
            ctx.obj["output"],
        )
    )


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
    typer.echo(
        handle_manual_tasks_list(
            status,
            request_id,
            ctx.obj["output"],
        )
    )


@manual_tasks_app.command(name="show")
def manual_tasks_show(
    ctx: typer.Context,
    task_id: int = typer.Argument(..., help="Task ID to show"),
) -> None:
    typer.echo(handle_manual_tasks_show(task_id, ctx.obj["output"]))


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
    typer.echo(
        handle_manual_tasks_complete(
            task_id,
            notes,
            ctx.obj["output"],
        )
    )


@app.command(name="solve-captcha")
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
) -> None:
    typer.echo(
        handle_solve_captcha(
            provider,
            api_key,
            site_key,
            page_url,
            ctx.obj["output"],
        )
    )


@app.command(name="generate-scheduler")
def generate_scheduler_cmd(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect if empty)",
    ),
    output_dir: str = typer.Option(
        "./schedules",
        "--output-dir",
        help="Output directory for generated files",
    ),
    tick_hour: int = typer.Option(
        10,
        "--tick-hour",
        help="Hour for daily tick (0-23)",
    ),
    tick_minute: int = typer.Option(
        0,
        "--tick-minute",
        help="Minute for daily tick (0-59)",
    ),
    poll_hours: str = typer.Option(
        "8,12,16,20",
        "--poll-hours",
        help="Comma-separated hours for poll-inbox",
    ),
    project_dir: str = typer.Option(
        "",
        "--project-dir",
        help="Project directory",
    ),
    openeraseme_bin: str = typer.Option(
        "",
        "--bin",
        help="Path to openeraseme binary",
    ),
    venv_activate: str = typer.Option(
        "",
        "--venv",
        help="Path to virtualenv activate script",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Preview files without writing",
    ),
) -> None:
    typer.echo(
        handle_generate_scheduler(
            platform,
            output_dir,
            tick_hour,
            tick_minute,
            poll_hours,
            project_dir,
            openeraseme_bin,
            venv_activate,
            dry_run,
            ctx.obj["output"],
        )
    )


@schedule_app.command()
def schedule_install(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
    tick_hour: int = typer.Option(
        10,
        "--tick-hour",
        help="Hour for daily tick (0-23)",
    ),
    tick_minute: int = typer.Option(
        0,
        "--tick-minute",
        help="Minute for daily tick (0-59)",
    ),
    yes: bool = typer.Option(
        False,
        "--yes",
        help="Skip confirmation prompt",
    ),
) -> None:
    typer.echo(
        handle_schedule_install(
            platform,
            tick_hour,
            tick_minute,
            yes,
            ctx.obj["output"],
        )
    )


@schedule_app.command(name="uninstall")
def schedule_uninstall(
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    typer.echo(handle_schedule_uninstall(platform))


@schedule_app.command()
def schedule_status(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    typer.echo(handle_schedule_status(platform, ctx.obj["output"]))


@app.command(name="generate-dashboard")
def generate_dashboard_cmd(
    ctx: typer.Context,
    output: str = typer.Option(
        "report.html",
        "--output",
        help="Output HTML file",
    ),
    auto_open: bool = typer.Option(
        False,
        "--open",
        help="Open in default browser",
    ),
    auto_refresh: int = typer.Option(
        0,
        "--auto-refresh",
        help="Auto-refresh interval in seconds (0 = none)",
    ),
) -> None:
    typer.echo(
        handle_generate_dashboard(
            output,
            auto_open,
            auto_refresh,
            ctx.obj["output"],
        )
    )


@app.command(name="generate-report")
def generate_report_cmd(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        None,
        "--campaign-id",
        help="Campaign ID to report on",
    ),
    format: str = typer.Option(
        "html",
        "--format",
        help="Output format: html, json, csv",
    ),
    output: str = typer.Option(
        "",
        "--output",
        help="Output file path (default: auto-generated)",
    ),
    all_campaigns: bool = typer.Option(
        False,
        "--all",
        help="Include all campaigns (not just specified one)",
    ),
) -> None:
    typer.echo(
        handle_generate_report(
            campaign_id,
            format,
            output,
            all_campaigns,
            ctx.obj["output"],
        )
    )


@app.command()
def db_init() -> None:
    typer.echo(handle_db_init())


if __name__ == "__main__":
    app()
