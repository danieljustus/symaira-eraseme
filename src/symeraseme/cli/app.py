"""Typer CLI application with rich-formatted output."""

from __future__ import annotations

import logging

import typer

from symeraseme.cli.commands.account_commands import (
    accounts_app,
    grant,
    init_profile,
    render_template,
    show_profile,
)
from symeraseme.cli.commands.inspection_commands import (
    brokers_app,
    doctor,
    events_app,
    requests_app,
    version,
)
from symeraseme.cli.commands.maintenance_commands import (
    db_init,
    export_cmd,
    generate_scheduler_cmd,
    registry_app,
    schedule_app,
    validate,
)
from symeraseme.cli.commands.monitoring_commands import (
    calendar,
    classify_reply,
    generate_dashboard_cmd,
    generate_rebuttal_cmd,
    generate_report_cmd,
    poll_inbox,
)
from symeraseme.cli.commands.plan_commands import (
    execute as execute_plan,
)
from symeraseme.cli.commands.plan_commands import (
    plan_app,
)
from symeraseme.cli.commands.plan_commands import (
    status as status_plan,
)
from symeraseme.cli.commands.plan_commands import (
    tick as tick_plan,
)
from symeraseme.cli.commands.web_form_commands import (
    auto_confirm_cmd,
    manual_tasks_app,
    run_web_form,
    solve_captcha_cmd,
)
from symeraseme.cli.console import OutputFormat
from symeraseme.core.db import close_connection

app = typer.Typer(
    name="symeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
    epilog=(
        "Quick Start:\n"
        "  1. symeraseme init-profile                       Create your identity profile\n"
        "  2. symeraseme brokers list --jurisdiction EU      Browse the broker registry\n"
        "  3. symeraseme plan create --campaign initial \\\n"
        "     --jurisdiction EU --max 10                     Plan a removal campaign\n"
        "  4. symeraseme plan execute --campaign initial \\\n"
        "     --batch-size 5 --yes                           Send opt-out requests\n"
        "  5. symeraseme plan tick                           Process deadlines & reminders\n"
        "  6. symeraseme plan status                         Check campaign progress\n"
        "\n"
        "Run 'symeraseme <command> --help' for detailed options."
    ),
)

app.add_typer(accounts_app, rich_help_panel="Account & Profile")
app.add_typer(plan_app, rich_help_panel="Planning & Execution")
app.add_typer(events_app, rich_help_panel="Inspection & Diagnostics")
app.add_typer(requests_app, rich_help_panel="Inspection & Diagnostics")
app.add_typer(manual_tasks_app, rich_help_panel="Web-form Automation")
app.add_typer(schedule_app, rich_help_panel="Maintenance")
app.add_typer(brokers_app, rich_help_panel="Inspection & Diagnostics")
app.add_typer(registry_app, rich_help_panel="Maintenance")


@app.callback()
def main(
    ctx: typer.Context,
    output: OutputFormat = OutputFormat.text,
    verbose: bool = typer.Option(False, "--verbose", "-v", help="Enable debug-level logging"),
) -> None:
    level = logging.DEBUG if verbose else logging.WARNING
    logging.basicConfig(
        level=level,
        format="%(levelname)s:%(name)s:%(message)s",
    )
    ctx.ensure_object(dict)
    ctx.obj["output"] = output
    ctx.call_on_close(close_connection)


# ── Account & Profile ─────────────────────────────────────────────────────
app.command(rich_help_panel="Account & Profile")(init_profile)
app.command(rich_help_panel="Account & Profile")(show_profile)
app.command(rich_help_panel="Account & Profile")(render_template)
app.command(rich_help_panel="Account & Profile")(grant)


@app.command(rich_help_panel="Account & Profile")
def revoke_llm_consent_cmd() -> None:
    """Revoke previously granted LLM PII consent."""
    from symeraseme.adapters.triage.scrubber import revoke_llm_consent

    revoke_llm_consent()
    typer.echo("LLM PII consent revoked.")


# ── Planning & Execution (deprecated top-level aliases) ───────────────────
@app.command(rich_help_panel="Planning & Execution", deprecated=True)
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
    consent_file: str = typer.Option(
        None,
        "--consent-file",
        help="Read consent token from a file (supports /dev/stdin for pipe input)",
    ),
    backend: str = typer.Option(
        None,
        "--backend",
        help="Execution backend: smtp (batch) or himalaya (CLI)",
    ),
) -> None:
    """[DEPRECATED] Use 'plan execute' instead."""
    typer.secho(
        "Warning: 'execute' is deprecated. Use 'plan execute' instead.",
        err=True,
        fg=typer.colors.YELLOW,
    )
    return execute_plan(
        ctx,
        campaign_id,
        account,
        batch_size,
        dry_run,
        yes,
        consent_token,
        consent_file,
        backend,
    )


@app.command(rich_help_panel="Planning & Execution", deprecated=True)
def tick(
    ctx: typer.Context,
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show actions without executing",
    ),
    batch_size: int = typer.Option(
        None,
        "--batch-size",
        help="Limit tick to N requests per run",
    ),
) -> None:
    """[DEPRECATED] Use 'plan tick' instead."""
    typer.secho(
        "Warning: 'tick' is deprecated. Use 'plan tick' instead.",
        err=True,
        fg=typer.colors.YELLOW,
    )
    return tick_plan(ctx, dry_run, batch_size)


@app.command(rich_help_panel="Planning & Execution", deprecated=True)
def status(
    ctx: typer.Context,
    campaign: str = typer.Option(
        None,
        "--campaign",
        help="Restrict to one campaign id (default: aggregate across all).",
    ),
) -> None:
    """[DEPRECATED] Use 'plan status' instead."""
    typer.secho(
        "Warning: 'status' is deprecated. Use 'plan status' instead.",
        err=True,
        fg=typer.colors.YELLOW,
    )
    return status_plan(ctx, campaign)


# ── Inspection & Diagnostics ──────────────────────────────────────────────
app.command(rich_help_panel="Inspection & Diagnostics")(version)
app.command(rich_help_panel="Inspection & Diagnostics")(doctor)

# ── Monitoring & Reports ─────────────────────────────────────────────────
app.command(name="poll-inbox", rich_help_panel="Monitoring & Reports")(poll_inbox)
app.command(name="classify-reply", rich_help_panel="Monitoring & Reports")(classify_reply)
app.command(name="generate-rebuttal", rich_help_panel="Monitoring & Reports")(generate_rebuttal_cmd)
app.command(name="generate-dashboard", rich_help_panel="Monitoring & Reports")(
    generate_dashboard_cmd
)
app.command(name="generate-report", rich_help_panel="Monitoring & Reports")(generate_report_cmd)
app.command(rich_help_panel="Monitoring & Reports")(calendar)

# ── Web-form Automation ───────────────────────────────────────────────────
app.command(name="run-web-form", rich_help_panel="Web-form Automation")(run_web_form)
app.command(name="auto-confirm", rich_help_panel="Web-form Automation")(auto_confirm_cmd)
app.command(name="solve-captcha", rich_help_panel="Web-form Automation")(solve_captcha_cmd)

# ── Maintenance ───────────────────────────────────────────────────────────
app.command(rich_help_panel="Maintenance")(db_init)
app.command(name="generate-scheduler", rich_help_panel="Maintenance")(generate_scheduler_cmd)
app.command(name="export", rich_help_panel="Maintenance")(export_cmd)
app.command(rich_help_panel="Maintenance")(validate)


if __name__ == "__main__":
    app()
