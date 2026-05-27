from __future__ import annotations

import base64
import contextlib
import hashlib
import json
import os
import secrets
import socket
import time
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlencode
from urllib.request import Request, urlopen

import keyring

SERVICE_NAME = "symeraseme-oauth2"
_STATE_FILE = "~/.local/share/symeraseme/oauth2_state.json"
_STATE_TTL = 300  # 5 minutes in seconds

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


class OAuth2StateError(OAuth2Error):
    pass


def _get_state_path() -> Path:
    path = Path(os.path.expanduser(_STATE_FILE))
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def _store_oauth2_state(state: str, provider: str) -> None:
    path = _get_state_path()
    existing: dict[str, dict[str, Any]] = {}
    if path.exists():
        with path.open() as f:
            existing = json.load(f)
    existing[state] = {"provider": provider, "expires_at": time.time() + _STATE_TTL}
    with path.open("w") as f:
        json.dump(existing, f)


def _validate_oauth2_state(state: str | None) -> None:
    path = _get_state_path()
    if not state:
        raise OAuth2StateError("Missing OAuth2 state parameter — possible CSRF attack.")
    if not path.exists():
        raise OAuth2StateError("No OAuth2 state stored — possible CSRF attack.")
    with path.open() as f:
        stored = json.load(f)
    record = stored.pop(state, None)
    if record is None:
        with path.open("w") as f:
            json.dump(stored, f)
        raise OAuth2StateError("OAuth2 state mismatch — possible CSRF attack.")
    if record.get("expires_at", 0) < time.time():
        with path.open("w") as f:
            json.dump(stored, f)
        raise OAuth2StateError("OAuth2 state expired — possible CSRF attack.")
    # Clean up the consumed state
    with path.open("w") as f:
        json.dump(stored, f)


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


def _generate_pkce_pair() -> tuple[str, str]:
    code_verifier = secrets.token_urlsafe(64)[:128]
    code_challenge = (
        base64.urlsafe_b64encode(hashlib.sha256(code_verifier.encode()).digest())
        .rstrip(b"=")
        .decode()
    )
    return code_verifier, code_challenge


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def authorize_url(provider: str, client_id: str, redirect_uri: str) -> tuple[str, str]:
    cfg = PROVIDER_CONFIGS.get(provider)
    if not cfg:
        msg = f"Unknown provider: {provider}. Supported: {list(PROVIDER_CONFIGS)}"
        raise OAuth2Error(msg)

    code_verifier, code_challenge = _generate_pkce_pair()
    state = secrets.token_urlsafe(16)
    _store_oauth2_state(state, provider)

    params = {
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "response_type": "code",
        "scope": cfg["scopes"],
        "access_type": "offline",
        "prompt": "consent",
        "state": state,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
    }
    return f"{cfg['auth_url']}?{urlencode(params)}", code_verifier


def exchange_code(
    provider: str,
    code: str,
    client_id: str,
    client_secret: str,
    redirect_uri: str,
    code_verifier: str = "",
) -> dict[str, Any]:
    cfg = PROVIDER_CONFIGS.get(provider)
    if not cfg:
        msg = f"Unknown provider: {provider}"
        raise OAuth2Error(msg)

    token_params: dict[str, str] = {
        "code": code,
        "client_id": client_id,
        "client_secret": client_secret,
        "redirect_uri": redirect_uri,
        "grant_type": "authorization_code",
    }
    if code_verifier:
        token_params["code_verifier"] = code_verifier
    data = urlencode(token_params).encode()

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


class CallbackHandler(BaseHTTPRequestHandler):
    auth_code: str = ""
    auth_error: str = ""

    def do_GET(self):
        params = parse_qs(self.path.split("?")[1] if "?" in self.path else "")
        try:
            state = params.get("state", [None])[0]
            _validate_oauth2_state(state)
        except OAuth2StateError as e:
            CallbackHandler.auth_error = str(e)
            self._respond(403, str(e))
            return
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


def run_local_server(port: int | None = None) -> tuple[str, str]:
    if port is None:
        port = find_free_port()
    redirect_uri = f"http://127.0.0.1:{port}/callback"
    server = HTTPServer(("127.0.0.1", port), CallbackHandler)
    CallbackHandler.auth_code = ""
    CallbackHandler.auth_error = ""
    server.timeout = 120
    while not CallbackHandler.auth_code:
        if CallbackHandler.auth_error:
            server.server_close()
            raise OAuth2StateError(CallbackHandler.auth_error)
        server.handle_request()
    server.server_close()
    return CallbackHandler.auth_code, redirect_uri
