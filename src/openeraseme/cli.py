from __future__ import annotations

from enum import StrEnum

import typer

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


class OutputFormat(StrEnum):
    text = "text"
    json = "json"


@app.callback()
def main(
    ctx: typer.Context,
    output: OutputFormat = OutputFormat.text,
) -> None:
    ctx.ensure_object(dict)
    ctx.obj["output"] = output


@app.command()
def version() -> None:
    from openeraseme import __version__

    typer.echo(f"OpenEraseMe v{__version__}")


@app.command()
def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    from openeraseme.core.identity import profile_exists, save_profile
    from openeraseme.registry.schema import IdentityProfile

    profile = IdentityProfile(full_name=full_name, email_addresses=[email])
    path = save_profile(profile)
    action = "Updated" if profile_exists() else "Created"
    typer.echo(f"{action} encrypted identity profile at {path}")


@app.command()
def show_profile() -> None:
    from openeraseme.core.identity import load_profile, profile_exists

    if not profile_exists():
        typer.echo("No identity profile found. Run 'openeraseme init-profile' first.")
        raise typer.Exit(1)

    profile = load_profile()
    typer.echo(f"Name:  {profile.full_name}")
    for e in profile.email_addresses:
        typer.echo(f"Email: {e}")
    for a in profile.addresses:
        typer.echo(f"Address: {a.street}, {a.city}, {a.country}")
    for j in profile.jurisdictions:
        typer.echo(f"Jurisdiction: {j}")


@accounts_app.command()
def add(
    provider: str = typer.Argument(help="Provider: gmail or outlook"),
    email: str = typer.Option(..., prompt=True, help="Email address"),
    client_id: str = typer.Option(..., prompt=True, help="OAuth2 client ID"),
    client_secret: str = typer.Option(
        ..., prompt=True, hide_input=True, help="OAuth2 client secret"
    ),
) -> None:
    from openeraseme.adapters.email.oauth2 import (
        _redirect_uri,
        _save_account_index,
        authorize_url,
        exchange_code,
        run_local_server,
        save_client_credentials,
        save_refresh_token,
    )

    save_client_credentials(email, client_id, client_secret)
    url = authorize_url(provider, client_id, _redirect_uri)
    typer.echo(f"Opening browser for OAuth2 authorization: {url}")
    import webbrowser

    webbrowser.open(url)

    typer.echo("Waiting for authorization callback on http://localhost:8899 ...")
    try:
        code = run_local_server()
    except TimeoutError:
        typer.echo(
            "Timed out waiting for authorization. "
            "You can also paste the code from the redirect URL."
        )
        code = typer.prompt("Authorization code")
    token_data = exchange_code(provider, code, client_id, client_secret, _redirect_uri)
    refresh_token = token_data.get("refresh_token", "")
    if refresh_token:
        save_refresh_token(email, refresh_token)
    _save_account_index(email, provider)
    typer.echo(f"Account {email} ({provider}) configured successfully.")


@accounts_app.command()
def list_cmd() -> None:
    from openeraseme.adapters.email.oauth2 import list_accounts

    accounts = list_accounts()
    if not accounts:
        typer.echo("No accounts configured.")
        return
    for acc in accounts:
        typer.echo(f"  {acc['email']} ({acc['provider']})")


@accounts_app.command()
def remove(
    email: str = typer.Argument(help="Email address to remove"),
) -> None:
    from openeraseme.adapters.email.oauth2 import _remove_from_index, delete_account

    delete_account(email)
    _remove_from_index(email)
    typer.echo(f"Account {email} removed.")


if __name__ == "__main__":
    app()
