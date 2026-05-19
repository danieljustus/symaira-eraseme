from __future__ import annotations

import contextlib
import json
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any
from urllib.parse import parse_qs, urlencode
from urllib.request import Request, urlopen

import keyring

SERVICE_NAME = "openeraseme-oauth2"

PROVIDER_CONFIGS: dict[str, dict[str, str]] = {
    "gmail": {
        "auth_url": "https://accounts.google.com/o/oauth2/v2/auth",
        "token_url": "https://oauth2.googleapis.com/token",
        "scopes": "https://mail.google.com/ https://www.googleapis.com/auth/gmail.send",
        "grant_type": "authorization_code",
    },
    "outlook": {
        "auth_url": "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        "token_url": "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        "scopes": (
            "https://outlook.office.com/IMAP.AccessAsUser.All "
            "https://outlook.office.com/SMTP.Send offline_access"
        ),
        "grant_type": "authorization_code",
    },
}


class OAuth2Error(Exception):
    pass


@dataclass
class AccountConfig:
    provider: str
    email: str
    client_id: str
    client_secret: str
    refresh_token: str = ""
    access_token: str = ""


def _keyring_key(email: str, name: str) -> str:
    return f"oauth2:{email}:{name}"


def save_client_credentials(email: str, client_id: str, client_secret: str) -> None:
    keyring.set_password(SERVICE_NAME, _keyring_key(email, "client_id"), client_id)
    keyring.set_password(SERVICE_NAME, _keyring_key(email, "client_secret"), client_secret)


def load_client_credentials(email: str) -> tuple[str, str]:
    client_id = keyring.get_password(SERVICE_NAME, _keyring_key(email, "client_id"))
    client_secret = keyring.get_password(SERVICE_NAME, _keyring_key(email, "client_secret"))
    if not client_id or not client_secret:
        msg = f"No OAuth2 credentials found for {email}. Run 'accounts add' first."
        raise OAuth2Error(msg)
    return client_id, client_secret


def save_refresh_token(email: str, token: str) -> None:
    keyring.set_password(SERVICE_NAME, _keyring_key(email, "refresh_token"), token)


def load_refresh_token(email: str) -> str:
    token = keyring.get_password(SERVICE_NAME, _keyring_key(email, "refresh_token"))
    if not token:
        msg = f"No refresh token found for {email}."
        raise OAuth2Error(msg)
    return token


def delete_account(email: str) -> None:
    for key_name in ("client_id", "client_secret", "refresh_token", "access_token"):
        with contextlib.suppress(keyring.errors.PasswordDeleteError):
            keyring.delete_password(SERVICE_NAME, _keyring_key(email, key_name))


def list_accounts() -> list[dict[str, str]]:
    # keyring doesn't natively list keys, so we read from a config index
    index_raw = keyring.get_password(SERVICE_NAME, "account_index")
    if not index_raw:
        return []
    try:
        return list(json.loads(index_raw))
    except (json.JSONDecodeError, TypeError):
        return []


def _save_account_index(email: str, provider: str) -> None:
    accounts = list_accounts()
    for acc in accounts:
        if acc.get("email") == email:
            acc["provider"] = provider
            break
    else:
        accounts.append({"email": email, "provider": provider})
    keyring.set_password(SERVICE_NAME, "account_index", json.dumps(accounts))


def _remove_from_index(email: str) -> None:
    accounts = [a for a in list_accounts() if a.get("email") != email]
    keyring.set_password(SERVICE_NAME, "account_index", json.dumps(accounts))


def authorize_url(provider: str, client_id: str, redirect_uri: str) -> str:
    cfg = PROVIDER_CONFIGS.get(provider)
    if not cfg:
        msg = f"Unknown provider: {provider}. Supported: {list(PROVIDER_CONFIGS)}"
        raise OAuth2Error(msg)

    params = {
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "response_type": "code",
        "scope": cfg["scopes"],
        "access_type": "offline",
        "prompt": "consent",
    }
    return f"{cfg['auth_url']}?{urlencode(params)}"


def exchange_code(
    provider: str,
    code: str,
    client_id: str,
    client_secret: str,
    redirect_uri: str,
) -> dict[str, Any]:
    cfg = PROVIDER_CONFIGS.get(provider)
    if not cfg:
        msg = f"Unknown provider: {provider}"
        raise OAuth2Error(msg)

    data = urlencode(
        {
            "code": code,
            "client_id": client_id,
            "client_secret": client_secret,
            "redirect_uri": redirect_uri,
            "grant_type": "authorization_code",
        }
    ).encode()

    req = Request(cfg["token_url"], data=data, method="POST")
    try:
        with urlopen(req, timeout=30) as resp:
            return dict(json.loads(resp.read()))
    except Exception as e:
        msg = f"Token exchange failed: {e}"
        raise OAuth2Error(msg) from e


def refresh_access_token(
    provider: str,
    client_id: str,
    client_secret: str,
    refresh_token: str,
) -> dict[str, Any]:
    cfg = PROVIDER_CONFIGS.get(provider)
    if not cfg:
        msg = f"Unknown provider: {provider}"
        raise OAuth2Error(msg)

    data = urlencode(
        {
            "refresh_token": refresh_token,
            "client_id": client_id,
            "client_secret": client_secret,
            "grant_type": "refresh_token",
        }
    ).encode()

    req = Request(cfg["token_url"], data=data, method="POST")
    try:
        with urlopen(req, timeout=30) as resp:
            return dict(json.loads(resp.read()))
    except Exception as e:
        msg = f"Token refresh failed: {e}"
        raise OAuth2Error(msg) from e


_redirect_uri = "http://localhost:8899/callback"


class CallbackHandler(BaseHTTPRequestHandler):
    auth_code: str = ""

    def do_GET(self):
        params = parse_qs(self.path.split("?")[1] if "?" in self.path else "")
        code_list = params.get("code", [])
        if code_list:
            CallbackHandler.auth_code = code_list[0]
            self._respond(200, "Authorization successful! You can close this window.")
        else:
            error = params.get("error", ["unknown"])[0]
            self._respond(400, f"Authorization failed: {error}")

    def _respond(self, status: int, body: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(body.encode())

    def log_message(self, fmt, *args):
        pass


def run_local_server() -> str:
    server = HTTPServer(("127.0.0.1", 8899), CallbackHandler)
    CallbackHandler.auth_code = ""
    server.timeout = 120
    while not CallbackHandler.auth_code:
        server.handle_request()
    server.server_close()
    return CallbackHandler.auth_code
