from __future__ import annotations

import json
import secrets
import time
from pathlib import Path
from unittest.mock import MagicMock, patch
from urllib.parse import urlparse

import pytest

from symeraseme.adapters.email.oauth2 import (
    OAuth2Error,
    OAuth2StateError,
    authorize_url,
    exchange_code,
)


@pytest.fixture(autouse=True)
def _fake_keyring():
    fake_store: dict[str, str] = {}

    def fake_set_password(service, username, password):
        fake_store[f"{service}:{username}"] = password

    def fake_get_password(service, username):
        return fake_store.get(f"{service}:{username}")

    def fake_delete_password(service, username):
        fake_store.pop(f"{service}:{username}", None)

    with (
        patch("symeraseme.adapters.email.oauth2.keyring.set_password", fake_set_password),
        patch("symeraseme.adapters.email.oauth2.keyring.get_password", fake_get_password),
        patch("symeraseme.adapters.email.oauth2.keyring.delete_password", fake_delete_password),
    ):
        yield


@pytest.fixture(autouse=True)
def _state_file(tmp_path: Path) -> None:
    state_file = tmp_path / "oauth2_state.json"
    with (
        patch("symeraseme.adapters.email.oauth2._get_state_path", return_value=state_file),
    ):
        yield


class TestAuthorizeUrl:
    def _parse_qs(self, url: str) -> dict[str, list[str]]:
        import urllib.parse
        return urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)

    def test_generates_gmail_url(self):
        url, _ = authorize_url("gmail", "my-client-id", "http://localhost:8899/callback")
        assert urlparse(url).hostname == "accounts.google.com"
        assert "client_id=my-client-id" in url
        assert "redirect_uri=http%3A%2F%2Flocalhost%3A8899%2Fcallback" in url
        assert "state=" in url

    def test_includes_pkce_params(self):
        url, verifier = authorize_url("gmail", "id", "http://localhost/")
        qs = self._parse_qs(url)
        assert "code_challenge" in qs
        assert qs["code_challenge_method"] == ["S256"]
        assert len(verifier) >= 43
        assert len(verifier) <= 128

    def test_code_challenge_is_sha256_of_verifier(self):
        import base64, hashlib
        url, verifier = authorize_url("gmail", "id", "http://localhost/")
        qs = self._parse_qs(url)
        expected = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        assert qs["code_challenge"][0] == expected

    def test_generates_outlook_url(self):
        url, _ = authorize_url("outlook", "my-client-id", "http://localhost:8899/callback")
        assert urlparse(url).hostname == "login.microsoftonline.com"
        assert "client_id=my-client-id" in url
        assert "state=" in url

    def test_unknown_provider_raises(self):
        with pytest.raises(OAuth2Error, match="Unknown"):
            authorize_url("unknown", "id", "http://localhost/")

    def test_state_is_random(self):
        url1, _ = authorize_url("gmail", "id1", "http://localhost:8899/callback")
        url2, _ = authorize_url("gmail", "id2", "http://localhost:8899/callback")
        assert url1 != url2
        s1 = self._parse_qs(url1).get("state", [""])[0]
        s2 = self._parse_qs(url2).get("state", [""])[0]
        assert s1 and s2
        assert s1 != s2

    def test_code_verifier_is_random(self):
        _, v1 = authorize_url("gmail", "id1", "http://localhost/")
        _, v2 = authorize_url("gmail", "id2", "http://localhost/")
        assert v1 != v2

    def test_state_stored_in_file(self):
        from symeraseme.adapters.email.oauth2 import _get_state_path

        url, _ = authorize_url("gmail", "id", "http://localhost/")
        state_path = _get_state_path()
        assert state_path.exists()
        raw = state_path.read_bytes()
        assert len(raw) > 0
        state_val = self._parse_qs(url).get("state")[0]
        stored = json.loads(state_path.read_text())
        assert state_val in stored
        assert stored[state_val]["provider"] == "gmail"
        assert stored[state_val]["expires_at"] > time.time()


class TestOAuth2StateValidation:
    def test_valid_state_passes(self):
        from symeraseme.adapters.email.oauth2 import _store_oauth2_state, _validate_oauth2_state

        state = secrets.token_urlsafe(16)
        _store_oauth2_state(state, "gmail")
        _validate_oauth2_state(state)  # should not raise

    def test_missing_state_raises(self):
        from symeraseme.adapters.email.oauth2 import _validate_oauth2_state

        with pytest.raises(OAuth2StateError, match="Missing"):
            _validate_oauth2_state(None)

    def test_empty_state_raises(self):
        from symeraseme.adapters.email.oauth2 import _validate_oauth2_state

        with pytest.raises(OAuth2StateError, match="Missing"):
            _validate_oauth2_state("")

    def test_mismatched_state_raises(self):
        from symeraseme.adapters.email.oauth2 import _store_oauth2_state, _validate_oauth2_state

        _store_oauth2_state("real-state", "gmail")
        with pytest.raises(OAuth2StateError, match="mismatch"):
            _validate_oauth2_state("fake-state")

    def test_expired_state_raises(self):
        from symeraseme.adapters.email.oauth2 import _get_state_path, _validate_oauth2_state

        state = secrets.token_urlsafe(16)
        state_path = _get_state_path()
        expired_record = {state: {"provider": "gmail", "expires_at": time.time() - 10}}
        state_path.write_text(json.dumps(expired_record))
        with pytest.raises(OAuth2StateError, match="expired"):
            _validate_oauth2_state(state)

    def test_state_cleaned_after_validation(self):
        from symeraseme.adapters.email.oauth2 import (
            _get_state_path,
            _store_oauth2_state,
            _validate_oauth2_state,
        )

        state = secrets.token_urlsafe(16)
        _store_oauth2_state(state, "gmail")
        _validate_oauth2_state(state)
        stored = json.loads(_get_state_path().read_text())
        assert state not in stored

    def test_no_state_file_raises(self):
        from symeraseme.adapters.email.oauth2 import _get_state_path, _validate_oauth2_state

        # Ensure the file doesn't exist
        state_path = _get_state_path()
        if state_path.exists():
            state_path.unlink()
        with pytest.raises(OAuth2StateError, match="No OAuth2 state stored"):
            _validate_oauth2_state("any-state")


class TestExchangeCode:
    @patch("symeraseme.adapters.email.oauth2.urlopen")
    def test_successful_exchange(self, mock_urlopen):
        mock_response = MagicMock()
        mock_response.read.return_value = b'{"access_token": "abc", "refresh_token": "xyz"}'
        mock_urlopen.return_value.__enter__.return_value = mock_response

        result = exchange_code(
            "gmail", "auth-code", "client-id", "secret", "http://localhost/", "test-verifier"
        )
        assert result["access_token"] == "abc"
        assert result["refresh_token"] == "xyz"

    @patch("symeraseme.adapters.email.oauth2.urlopen")
    def test_code_verifier_included_in_request(self, mock_urlopen):
        mock_response = MagicMock()
        mock_response.read.return_value = b'{"access_token": "abc"}'
        mock_urlopen.return_value.__enter__.return_value = mock_response

        exchange_code("gmail", "code", "id", "secret", "http://localhost/", "my-verifier")
        call_data = mock_urlopen.call_args[0][0].data.decode()
        assert "code_verifier=my-verifier" in call_data

    @patch("symeraseme.adapters.email.oauth2.urlopen")
    def test_code_verifier_omitted_when_empty(self, mock_urlopen):
        mock_response = MagicMock()
        mock_response.read.return_value = b'{"access_token": "abc"}'
        mock_urlopen.return_value.__enter__.return_value = mock_response

        exchange_code("gmail", "code", "id", "secret", "http://localhost/")
        call_data = mock_urlopen.call_args[0][0].data.decode()
        assert "code_verifier" not in call_data

    @patch("symeraseme.adapters.email.oauth2.urlopen")
    def test_exchange_failure_raises(self, mock_urlopen):
        from urllib.error import URLError

        mock_urlopen.side_effect = URLError("connection refused")

        with pytest.raises(OAuth2Error, match="connection refused"):
            exchange_code("gmail", "bad-code", "id", "secret", "http://localhost/")


class TestCredentialsStorage:
    def test_save_and_load_roundtrip(self):
        from symeraseme.adapters.email.oauth2 import (
            delete_account,
            load_client_credentials,
            save_client_credentials,
        )

        save_client_credentials("test@example.com", "my-id", "my-secret")
        cid, csecret = load_client_credentials("test@example.com")
        assert cid == "my-id"
        assert csecret == "my-secret"
        delete_account("test@example.com")

    def test_load_nonexistent_raises(self):
        from symeraseme.adapters.email.oauth2 import load_client_credentials

        with pytest.raises(OAuth2Error, match="No OAuth2 credentials"):
            load_client_credentials("nonexistent@example.com")


class TestRefreshToken:
    def test_save_and_load(self):
        from symeraseme.adapters.email.oauth2 import (
            delete_account,
            load_refresh_token,
            save_refresh_token,
        )

        save_refresh_token("test@example.com", "my-refresh-token")
        token = load_refresh_token("test@example.com")
        assert token == "my-refresh-token"
        delete_account("test@example.com")

    def test_load_nonexistent_raises(self):
        from symeraseme.adapters.email.oauth2 import load_refresh_token

        with pytest.raises(OAuth2Error, match="No refresh token"):
            load_refresh_token("nonexistent@example.com")


class TestAccountIndex:
    def test_list_empty(self):
        from symeraseme.adapters.email.oauth2 import list_accounts

        assert list_accounts() == []

    def test_add_and_list(self):
        from symeraseme.adapters.email.oauth2 import (
            _remove_from_index,
            _save_account_index,
            list_accounts,
        )

        _save_account_index("a@b.com", "gmail")
        _save_account_index("c@d.com", "outlook")
        accounts = list_accounts()
        assert len(accounts) == 2
        assert accounts[0]["email"] == "a@b.com"
        _remove_from_index("a@b.com")
        _remove_from_index("c@d.com")
        assert list_accounts() == []
