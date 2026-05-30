"""Account & Profile commands."""

from __future__ import annotations

import typer

from symeraseme.cli.console import (
    console,
    print_panel,
    print_success,
    print_table,
)
from symeraseme.core.identity import load_profile, profile_exists, save_profile
from symeraseme.core.templating import render_template as _render
from symeraseme.registry.schema import IdentityProfile
from symeraseme.services.consent import handle_grant

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
    import webbrowser

    from symeraseme.adapters.email.oauth2 import (
        _save_account_index,
        authorize_url,
        exchange_code,
        find_free_port,
        run_local_server,
        save_client_credentials,
        save_refresh_token,
    )

    save_client_credentials(email, client_id, client_secret)
    port = find_free_port()
    redirect_uri = f"http://127.0.0.1:{port}/callback"
    url, code_verifier = authorize_url(provider, client_id, redirect_uri)
    typer.echo(f"Opening browser for OAuth2 authorization: {url}")
    webbrowser.open(url)

    typer.echo(f"Waiting for authorization callback on {redirect_uri} ...")
    try:
        code, _ = run_local_server(port=port)
    except TimeoutError:
        typer.echo(
            "Timed out waiting for authorization. "
            "You can also paste the code from the redirect URL."
        )
        code = typer.prompt("Authorization code")
    token_data = exchange_code(
        provider, code, client_id, client_secret, redirect_uri, code_verifier
    )
    refresh_token = token_data.get("refresh_token", "")
    if refresh_token:
        save_refresh_token(email, refresh_token)
    _save_account_index(email, provider)
    print_success(f"Account {email} ({provider}) configured successfully.")


@accounts_app.command(name="list")
def list_cmd() -> None:
    from symeraseme.adapters.email.oauth2 import list_accounts

    accounts = list_accounts()
    if not accounts:
        console.print("No accounts configured.", markup=False, soft_wrap=True)
        return
    rows = [[a["email"], a["provider"]] for a in accounts]
    print_table("Accounts", ["Email", "Provider"], rows)


@accounts_app.command()
def remove(email: str = typer.Argument(help="Email address to remove")) -> None:
    from symeraseme.adapters.email.oauth2 import _remove_from_index, delete_account

    delete_account(email)
    _remove_from_index(email)
    console.print(f"Account {email} removed.", markup=False, soft_wrap=True)


def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    profile = IdentityProfile(full_name=full_name, email_addresses=[email])
    path = save_profile(profile)
    action = "Updated" if profile_exists() else "Created"
    print_success(f"{action} encrypted identity profile at {path}")


def show_profile() -> None:
    if not profile_exists():
        from symeraseme.cli.console import render_error

        render_error("No identity profile found. Run 'symeraseme init-profile' first.")

    profile = load_profile()
    lines = [f"Name:  {profile.full_name}"]
    for e in profile.email_addresses:
        lines.append(f"Email: {e}")
    for a in profile.addresses:
        lines.append(f"Address: {a.street}, {a.city}, {a.country}")
    for j in profile.jurisdictions:
        lines.append(f"Jurisdiction: {j}")
    info = "\n".join(lines)
    print_panel("Profile", info)


def render_template(
    template: str = typer.Argument(
        help="Template name (e.g. gdpr-art17.de.md.j2)",
    ),
    broker_name: str = typer.Option("", help="Name of the data broker"),
    broker_website: str = typer.Option("", help="Broker website URL"),
) -> None:
    prof = load_profile() if profile_exists() else None
    result = _render(
        template,
        profile=prof,
        broker_name=broker_name,
        broker_website=broker_website,
    )
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
    )
    from symeraseme.cli.console import render_result

    render_result(ctx.obj["output"], result)
