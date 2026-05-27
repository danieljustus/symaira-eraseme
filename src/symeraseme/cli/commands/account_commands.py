"""Account & Profile commands."""

from __future__ import annotations

import typer

from symeraseme.cli.console import (
    console,
    print_panel,
    print_success,
    print_table,
)
from symeraseme.services.account import (
    handle_account_add,
    handle_account_list,
    handle_account_remove,
)
from symeraseme.services.consent import handle_grant
from symeraseme.services.profile import (
    handle_init_profile,
    handle_render_template,
    handle_show_profile,
)

accounts_app = typer.Typer(
    name="accounts",
    help="Manage email accounts (OAuth2 setup, list, remove)",
    no_args_is_help=True,
)


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
    result = handle_account_add(provider, email, client_id, client_secret)
    print_success(result)


@accounts_app.command(name="list")
def list_cmd() -> None:
    result = handle_account_list()
    if result.startswith("No"):
        console.print(result, markup=False, soft_wrap=True)
        return
    rows = []
    for line in result.strip().split("\n"):
        line = line.strip()
        if line:
            rows.append(line.split(None, 1))
    if rows:
        print_table("Accounts", ["Email", "Provider"], rows)
    else:
        console.print(result, markup=False, soft_wrap=True)


@accounts_app.command()
def remove(email: str = typer.Argument(help="Email address to remove")) -> None:
    result = handle_account_remove(email)
    console.print(result, markup=False, soft_wrap=True)


def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    result = handle_init_profile(full_name, email)
    print_success(result)


def show_profile() -> None:
    try:
        result = handle_show_profile()
    except typer.Exit:
        raise
    info = "\n".join(line.strip() for line in result.split("\n") if line.strip())
    print_panel("Profile", info)


def render_template(
    template: str = typer.Argument(
        help="Template name (e.g. gdpr-art17.de.md.j2)",
    ),
    broker_name: str = typer.Option("", help="Name of the data broker"),
    broker_website: str = typer.Option("", help="Broker website URL"),
) -> None:
    result = handle_render_template(template, broker_name, broker_website)
    console.print(result, markup=False, soft_wrap=True)


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
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show token without creating it",
    ),
) -> None:
    result = handle_grant(
        command,
        ttl,
        revoke,
        revoke_all,
        list_tokens,
        dry_run,
        ctx.obj["output"],
    )
    from symeraseme.cli.console import render_result

    render_result(ctx.obj["output"], result)
