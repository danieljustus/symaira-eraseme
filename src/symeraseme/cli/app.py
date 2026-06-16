"""Typer CLI application with rich-formatted output."""

from __future__ import annotations

import logging

import typer

from symeraseme import __version__
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
    db_migrate,
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
from symeraseme.cli.commands.plan_commands import plan_app
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
        "  2. symeraseme brokers list --law GDPR             Browse the broker registry\n"
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


def _print_version(value: bool) -> None:
    if value:
        print(f"Symaira EraseMe v{__version__}")
        raise typer.Exit()


@app.callback()
def main(
    ctx: typer.Context,
    output: OutputFormat = OutputFormat.text,
    verbose: int = typer.Option(
        0,
        "--verbose",
        "-v",
        help="Increase verbosity (-v for INFO, -vv for DEBUG)",
        count=True,
    ),
    version: bool = typer.Option(
        False,
        "--version",
        "-V",
        help="Show version and exit",
        is_eager=True,
        callback=_print_version,
    ),
) -> None:
    if verbose >= 2:
        level = logging.DEBUG
    elif verbose == 1:
        level = logging.INFO
    else:
        level = logging.WARNING
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
app.command(name="db-migrate", rich_help_panel="Maintenance")(db_migrate)
app.command(name="generate-scheduler", rich_help_panel="Maintenance")(generate_scheduler_cmd)
app.command(name="export", rich_help_panel="Maintenance")(export_cmd)
app.command(rich_help_panel="Maintenance")(validate)


if __name__ == "__main__":
    app()
