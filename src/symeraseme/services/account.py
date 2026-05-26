"""Account management CLI handlers."""

from __future__ import annotations

import webbrowser

import typer

from symeraseme.adapters.email.oauth2 import (
    _redirect_uri,
    _remove_from_index,
    _save_account_index,
    authorize_url,
    delete_account,
    exchange_code,
    list_accounts,
    run_local_server,
    save_client_credentials,
    save_refresh_token,
)


def handle_account_add(
    provider: str,
    email: str,
    client_id: str,
    client_secret: str,
) -> str:
    save_client_credentials(email, client_id, client_secret)
    url = authorize_url(provider, client_id, _redirect_uri)
    typer.echo(f"Opening browser for OAuth2 authorization: {url}")
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
    return f"Account {email} ({provider}) configured successfully."


def handle_account_list() -> str:
    accounts = list_accounts()
    if not accounts:
        return "No accounts configured."
    return "\n".join(f"  {a['email']} ({a['provider']})" for a in accounts)


def handle_account_remove(email: str) -> str:
    delete_account(email)
    _remove_from_index(email)
    return f"Account {email} removed."
