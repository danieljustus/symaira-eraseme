"""Extended tests for ``AnthropicClient`` — cost, API calls, availability."""

from __future__ import annotations

import sys
from unittest.mock import MagicMock, patch

import pytest


@pytest.fixture(autouse=True)
def _restore_anthropic_module():
    """Restore the real ``anthropic`` module after tests that fake it in sys.modules."""
    original = sys.modules.get("anthropic")
    yield
    if original is not None:
        sys.modules["anthropic"] = original
    else:
        sys.modules.pop("anthropic", None)


def _mock_anthropic_module():
    """Create and inject a mock ``anthropic`` module into sys.modules."""
    mock_anthropic = MagicMock()
    mock_anthropic.Anthropic = MagicMock()

    mock_anthropic.RateLimitError = type("RateLimitError", (Exception,), {})
    mock_anthropic.APIStatusError = type("APIStatusError", (Exception,), {})
    mock_anthropic.APIConnectionError = type("APIConnectionError", (Exception,), {})

    # Remove any cached import so client re-imports
    if "anthropic" in sys.modules:
        del sys.modules["anthropic"]

    sys.modules["anthropic"] = mock_anthropic
    return mock_anthropic


class TestAnthropicClientInit:
    """Coverage for ``AnthropicClient.__init__``."""

    def test_default_initialization(self):
        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient()
        assert client.model == "claude-sonnet-4-6"
        assert client.max_retries == 3
        assert client.cost_tracker == []
        assert client._api_key is None
        assert client._client is None

    def test_custom_initialization(self):
        from symeraseme.llm.anthropic_client import AnthropicClient

        tracker: list = []
        client = AnthropicClient(
            api_key="sk-custom",
            model="claude-opus-4-8",
            max_retries=5,
            cost_tracker=tracker,
        )
        assert client.model == "claude-opus-4-8"
        assert client.max_retries == 5
        assert client.cost_tracker is tracker
        assert client._api_key == "sk-custom"


class TestAnthropicClientProperty:
    """Coverage for the ``client`` property — lazy initialization."""

    def test_lazy_initialization(self):
        mock_anthropic = _mock_anthropic_module()
        mock_instance = MagicMock()
        mock_anthropic.Anthropic.return_value = mock_instance

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert client._client is None

        result = client.client

        assert result is mock_instance
        assert client._client is mock_instance

    def test_cached_after_first_access(self):
        mock_anthropic = _mock_anthropic_module()
        mock_instance = MagicMock()
        mock_anthropic.Anthropic.return_value = mock_instance

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        first = client.client
        second = client.client

        assert first is second
        mock_anthropic.Anthropic.assert_called_once()

    def test_passes_api_key(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-secret-key")
        _ = client.client

        mock_anthropic.Anthropic.assert_called_once_with(api_key="sk-secret-key")


class TestAnthropicClientIsAvailable:
    """Coverage for ``is_available`` — all code paths."""

    def test_sdk_not_installed_returns_false(self):
        """When the anthropic SDK is not installed, is_available returns False."""
        # Remove anthropic from sys.modules to force import error
        was_in_modules = "anthropic" in sys.modules
        saved = sys.modules.pop("anthropic", None)

        import builtins

        real_import = builtins.__import__

        def fake_import(name, *args, **kwargs):
            if name == "anthropic":
                raise ImportError("No module named 'anthropic'")
            return real_import(name, *args, **kwargs)

        try:
            with patch.object(builtins, "__import__", side_effect=fake_import):
                # Clear any cached imports in the client module
                import symeraseme.llm.anthropic_client as ac_mod

                if "anthropic" in ac_mod.__dict__:
                    del ac_mod.__dict__["anthropic"]

                from symeraseme.llm.anthropic_client import AnthropicClient

                client = AnthropicClient(api_key="sk-test")
                assert client.is_available() is False
        finally:
            if saved is not None:
                sys.modules["anthropic"] = saved

    def test_returns_true_with_api_key(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert client.is_available() is True

    def test_returns_false_with_empty_api_key(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="")
        assert client.is_available() is False

    def test_uses_env_var_when_no_api_key(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key=None)
        with patch.dict("os.environ", {"ANTHROPIC_API_KEY": "sk-from-env"}, clear=True):
            assert client.is_available() is True
        assert client._api_key == "sk-from-env"

    def test_env_var_resolves_vault_secret(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient
        import symeraseme.llm.anthropic_client as ac_mod

        client = AnthropicClient(api_key=None)
        with (
            patch.dict("os.environ", {"ANTHROPIC_API_KEY": "vault://anthropic/key"}, clear=True),
            patch.object(ac_mod, "resolve_secret", return_value="sk-resolved"),
        ):
            assert client.is_available() is True
        assert client._api_key == "sk-resolved"

    def test_env_var_secret_resolution_failure(self):
        mock_anthropic = _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient
        from symeraseme.core.secrets import SecretResolutionError
        import symeraseme.llm.anthropic_client as ac_mod

        client = AnthropicClient(api_key=None)
        with (
            patch.dict("os.environ", {"ANTHROPIC_API_KEY": "vault://anthropic/key"}, clear=True),
            patch.object(ac_mod, "resolve_secret", side_effect=SecretResolutionError("nope")),
        ):
            assert client.is_available() is False


class TestAnthropicClientComputeCost:
    """Coverage for ``_compute_cost`` — pricing logic."""

    def make_record(self, model="claude-sonnet-4-6", input_t=0, output_t=0,
                    cache_create=0, cache_read=0):
        from symeraseme.llm.anthropic_client import UsageRecord

        return UsageRecord(
            model=model,
            input_tokens=input_t,
            output_tokens=output_t,
            cache_creation_tokens=cache_create,
            cache_read_tokens=cache_read,
        )

    def _get_client(self, model="claude-sonnet-4-6"):
        from symeraseme.llm.anthropic_client import AnthropicClient

        return AnthropicClient(api_key="sk-test", model=model)

    def test_zero_cost(self):
        client = self._get_client()
        record = self.make_record(input_t=0, output_t=0)
        assert client._compute_cost(record) == 0.0

    def test_input_output_cost(self):
        client = self._get_client()
        record = self.make_record(input_t=1000, output_t=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.0105)

    def test_cache_costs(self):
        client = self._get_client()
        record = self.make_record(input_t=500, output_t=200, cache_create=1000, cache_read=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.0084)

    def test_unknown_model_uses_default_pricing(self):
        client = self._get_client(model="unknown-model")
        record = self.make_record(model="unknown-model", input_t=1000, output_t=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.0105)

    def test_opus_pricing(self):
        client = self._get_client(model="claude-opus-4-8")
        record = self.make_record(model="claude-opus-4-8", input_t=1000, output_t=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.0525)

    def test_haiku_pricing(self):
        client = self._get_client(model="claude-haiku-4-5")
        record = self.make_record(model="claude-haiku-4-5", input_t=1000, output_t=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.0028)

    def test_fable_pricing(self):
        client = self._get_client(model="claude-fable-5")
        record = self.make_record(model="claude-fable-5", input_t=1000, output_t=500)
        cost = client._compute_cost(record)
        assert cost == pytest.approx(0.000875)


class TestAnthropicClientSupportsPromptCaching:
    """Coverage for ``_supports_prompt_caching``."""

    def test_returns_true(self):
        _mock_anthropic_module()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert client._supports_prompt_caching() is True


class TestAnthropicClientCallApi:
    """Coverage for ``_call_api`` — all code paths."""

    def _make_mocks(self):
        """Create and inject mock anthropic module, return key objects."""
        mock_anthropic = MagicMock()
        mock_anthropic.Anthropic = MagicMock()
        mock_anthropic.RateLimitError = type("RateLimitError", (Exception,), {})
        mock_anthropic.APIStatusError = type("APIStatusError", (Exception,), {})
        mock_anthropic.APIConnectionError = type("APIConnectionError", (Exception,), {})

        mock_client = MagicMock()
        mock_usage = MagicMock()
        mock_usage.input_tokens = 50
        mock_usage.output_tokens = 25
        mock_usage.cache_creation_input_tokens = 0
        mock_usage.cache_read_input_tokens = 0

        mock_content = MagicMock()
        mock_content.text = "This is the response."

        mock_message = MagicMock()
        mock_message.content = [mock_content]
        mock_message.usage = mock_usage

        mock_client.messages.create.return_value = mock_message
        mock_anthropic.Anthropic.return_value = mock_client

        # Inject mock into sys.modules
        if "anthropic" in sys.modules:
            del sys.modules["anthropic"]
        sys.modules["anthropic"] = mock_anthropic

        return mock_anthropic, mock_client, mock_message

    def test_successful_call(self):
        mock_anthropic, mock_client, _ = self._make_mocks()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")

        text, record = client._call_api(
            system_prompt="You are a helpful assistant.",
            user_prompt="Hello!",
            max_tokens=256,
            temperature=0.0,
            cache_key=None,
        )

        assert text == "This is the response."
        assert record.input_tokens == 50
        assert record.output_tokens == 25
        assert record.cost > 0

    def test_successful_call_with_cache_key(self):
        mock_anthropic, mock_client, _ = self._make_mocks()

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")

        text, _ = client._call_api(
            system_prompt="You are cached.",
            user_prompt="Hello!",
            max_tokens=256,
            temperature=0.0,
            cache_key="my-cache-key",
        )

        assert text == "This is the response."
        call_kwargs = mock_client.messages.create.call_args[1]
        assert isinstance(call_kwargs["system"], list)
        assert "cache_control" in call_kwargs["system"][0]

    def test_rate_limit_error(self):
        mock_anthropic, mock_client, _ = self._make_mocks()
        mock_client.messages.create.side_effect = mock_anthropic.RateLimitError("Too fast")

        from symeraseme.llm.anthropic_client import AnthropicClient, AnthropicClientRateLimitError

        client = AnthropicClient(api_key="sk-test")
        with pytest.raises(AnthropicClientRateLimitError):
            client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=256,
                temperature=0.0,
                cache_key=None,
            )

    def test_api_status_error(self):
        mock_anthropic, mock_client, _ = self._make_mocks()
        mock_client.messages.create.side_effect = mock_anthropic.APIStatusError("Bad request")

        from symeraseme.llm.anthropic_client import AnthropicClient, AnthropicClientError

        client = AnthropicClient(api_key="sk-test")
        with pytest.raises(AnthropicClientError):
            client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=256,
                temperature=0.0,
                cache_key=None,
            )

    def test_api_connection_error(self):
        mock_anthropic, mock_client, _ = self._make_mocks()
        mock_client.messages.create.side_effect = mock_anthropic.APIConnectionError("Connection failed")

        from symeraseme.llm.anthropic_client import AnthropicClient, AnthropicClientError

        client = AnthropicClient(api_key="sk-test")
        with pytest.raises(AnthropicClientError):
            client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=256,
                temperature=0.0,
                cache_key=None,
            )

    def test_response_concatenates_multiple_content_blocks(self):
        mock_anthropic, mock_client, _ = self._make_mocks()

        block1 = MagicMock()
        block1.text = "Part one. "
        block2 = MagicMock()
        block2.text = "Part two."
        mock_client.messages.create.return_value.content = [block1, block2]

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")

        text, _ = client._call_api(
            system_prompt="sys",
            user_prompt="user",
            max_tokens=256,
            temperature=0.0,
            cache_key=None,
        )

        assert text == "Part one. Part two."

    def test_blocks_without_text_are_skipped(self):
        mock_anthropic, mock_client, _ = self._make_mocks()

        block1 = MagicMock(spec=[])  # no text attribute
        block2 = MagicMock()
        block2.text = "Only this."
        mock_client.messages.create.return_value.content = [block1, block2]

        from symeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")

        text, _ = client._call_api(
            system_prompt="sys",
            user_prompt="user",
            max_tokens=256,
            temperature=0.0,
            cache_key=None,
        )

        assert text == "Only this."


class TestAnthropicClientExceptionClasses:
    """Additional exception hierarchy tests beyond the basic ones."""

    def test_anthropic_client_error_str(self):
        from symeraseme.llm.anthropic_client import AnthropicClientError

        err = AnthropicClientError("custom error message")
        assert str(err) == "custom error message"

    def test_anthropic_rate_limit_error_str(self):
        from symeraseme.llm.anthropic_client import AnthropicClientRateLimitError

        err = AnthropicClientRateLimitError("rate limited")
        assert str(err) == "rate limited"
