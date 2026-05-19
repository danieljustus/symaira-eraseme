from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest

from openeraseme.adapters.email.oauth2 import (
    OAuth2Error,
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
        patch("openeraseme.adapters.email.oauth2.keyring.set_password", fake_set_password),
        patch("openeraseme.adapters.email.oauth2.keyring.get_password", fake_get_password),
        patch("openeraseme.adapters.email.oauth2.keyring.delete_password", fake_delete_password),
    ):
        yield


class TestAuthorizeUrl:
    def test_generates_gmail_url(self):
        url = authorize_url("gmail", "my-client-id", "http://localhost:8899/callback")
        assert "accounts.google.com" in url
        assert "client_id=my-client-id" in url
        assert "redirect_uri=http%3A%2F%2Flocalhost%3A8899%2Fcallback" in url

    def test_generates_outlook_url(self):
        url = authorize_url("outlook", "my-client-id", "http://localhost:8899/callback")
        assert "login.microsoftonline.com" in url
        assert "client_id=my-client-id" in url

    def test_unknown_provider_raises(self):
        with pytest.raises(OAuth2Error, match="Unknown"):
            authorize_url("unknown", "id", "http://localhost/")


class TestExchangeCode:
    @patch("openeraseme.adapters.email.oauth2.urlopen")
    def test_successful_exchange(self, mock_urlopen):
        mock_response = MagicMock()
        mock_response.read.return_value = b'{"access_token": "abc", "refresh_token": "xyz"}'
        mock_urlopen.return_value.__enter__.return_value = mock_response

        result = exchange_code("gmail", "auth-code", "client-id", "secret", "http://localhost/")
        assert result["access_token"] == "abc"
        assert result["refresh_token"] == "xyz"

    @patch("openeraseme.adapters.email.oauth2.urlopen")
    def test_exchange_failure_raises(self, mock_urlopen):
        from urllib.error import URLError

        mock_urlopen.side_effect = URLError("connection refused")

        with pytest.raises(OAuth2Error, match="connection refused"):
            exchange_code("gmail", "bad-code", "id", "secret", "http://localhost/")


class TestCredentialsStorage:
    def test_save_and_load_roundtrip(self):
        from openeraseme.adapters.email.oauth2 import (
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
        from openeraseme.adapters.email.oauth2 import load_client_credentials

        with pytest.raises(OAuth2Error, match="No OAuth2 credentials"):
            load_client_credentials("nonexistent@example.com")


class TestRefreshToken:
    def test_save_and_load(self):
        from openeraseme.adapters.email.oauth2 import (
            delete_account,
            load_refresh_token,
            save_refresh_token,
        )

        save_refresh_token("test@example.com", "my-refresh-token")
        token = load_refresh_token("test@example.com")
        assert token == "my-refresh-token"
        delete_account("test@example.com")

    def test_load_nonexistent_raises(self):
        from openeraseme.adapters.email.oauth2 import load_refresh_token

        with pytest.raises(OAuth2Error, match="No refresh token"):
            load_refresh_token("nonexistent@example.com")


class TestAccountIndex:
    def test_list_empty(self):
        from openeraseme.adapters.email.oauth2 import list_accounts

        assert list_accounts() == []

    def test_add_and_list(self):
        from openeraseme.adapters.email.oauth2 import (
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
