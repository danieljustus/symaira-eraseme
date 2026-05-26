from __future__ import annotations

import sys
from types import ModuleType
from unittest.mock import MagicMock, patch

import pytest

from openeraseme.llm.openai_client import OpenAIClient
from openeraseme.llm.protocol import LLMClientError, LLMClientRateLimitError, UsageRecord


@pytest.fixture(autouse=True)
def fake_openai_module():
    """Provide a fake openai module so tests run without the real SDK."""
    fake_openai = ModuleType("openai")
    fake_openai.OpenAI = MagicMock()
    fake_openai.RateLimitError = type("RateLimitError", (Exception,), {})
    fake_openai.APIStatusError = type("APIStatusError", (Exception,), {})
    fake_openai.APIConnectionError = type("APIConnectionError", (Exception,), {})
    sys.modules["openai"] = fake_openai
    yield fake_openai
    del sys.modules["openai"]


class TestOpenAIClientIsAvailable:
    def test_available_with_api_key(self, fake_openai_module):
        client = OpenAIClient(api_key="sk-test")
        assert client.is_available()

    def test_not_available_without_key(self):
        with patch.dict("os.environ", {}, clear=True):
            client = OpenAIClient()
        assert not client.is_available()

    def test_available_with_env_var(self, fake_openai_module):
        with patch.dict("os.environ", {"OPENAI_API_KEY": "sk-env"}, clear=True):
            client = OpenAIClient()
            assert client.is_available()

    def test_not_available_when_sdk_missing(self):
        with (
            patch.dict("sys.modules", {"openai": None}, clear=False),
            patch.dict("os.environ", {"OPENAI_API_KEY": "sk-test"}, clear=True),
        ):
            client = OpenAIClient()
            assert not client.is_available()


class TestOpenAIClientClassify:
    def test_classify_success(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 10
        mock_usage.completion_tokens = 5

        mock_choice = MagicMock()
        mock_choice.message.content = "positive"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.return_value = mock_response
        fake_openai_module.OpenAI.return_value = mock_client

        client = OpenAIClient(api_key="sk-test")
        text, record = client.classify(
            system_prompt="You are a classifier.",
            user_prompt="Classify this.",
        )

        assert text == "positive"
        assert isinstance(record, UsageRecord)
        assert record.input_tokens == 10
        assert record.output_tokens == 5
        assert record.model == "gpt-4o"

    def test_classify_with_custom_model(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 20
        mock_usage.completion_tokens = 10

        mock_choice = MagicMock()
        mock_choice.message.content = "negative"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.return_value = mock_response
        fake_openai_module.OpenAI.return_value = mock_client

        client = OpenAIClient(api_key="sk-test", model="gpt-4o-mini")
        text, record = client.classify(
            system_prompt="You are a classifier.",
            user_prompt="Classify this.",
        )

        assert text == "negative"
        assert record.model == "gpt-4o-mini"

    def test_classify_raises_when_not_available(self):
        client = OpenAIClient()
        with pytest.raises(LLMClientError, match="not available"):
            client.classify("sys", "user")


class TestOpenAIClientRetry:
    def test_retry_on_rate_limit_then_success(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 5
        mock_usage.completion_tokens = 3

        mock_choice = MagicMock()
        mock_choice.message.content = "ok"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.side_effect = [
            fake_openai_module.RateLimitError("rate limited"),
            mock_response,
        ]
        fake_openai_module.OpenAI.return_value = mock_client

        with patch("openeraseme.llm.openai_client.time.sleep") as mock_sleep:
            client = OpenAIClient(api_key="sk-test", max_retries=3)
            text, record = client.classify("sys", "user")

        assert text == "ok"
        assert mock_client.chat.completions.create.call_count == 2
        mock_sleep.assert_called_once()

    def test_retry_exhausted_raises_rate_limit_error(self, fake_openai_module):
        mock_client = MagicMock()
        mock_client.chat.completions.create.side_effect = (
            fake_openai_module.RateLimitError("rate limited")
        )
        fake_openai_module.OpenAI.return_value = mock_client

        with patch("openeraseme.llm.openai_client.time.sleep") as mock_sleep:
            client = OpenAIClient(api_key="sk-test", max_retries=2)
            with pytest.raises(LLMClientRateLimitError):
                client.classify("sys", "user")

        assert mock_client.chat.completions.create.call_count == 2
        assert mock_sleep.call_count == 1

    def test_retry_on_api_error_then_success(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 5
        mock_usage.completion_tokens = 3

        mock_choice = MagicMock()
        mock_choice.message.content = "ok"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.side_effect = [
            fake_openai_module.APIStatusError("server error"),
            mock_response,
        ]
        fake_openai_module.OpenAI.return_value = mock_client

        with patch("openeraseme.llm.openai_client.time.sleep") as mock_sleep:
            client = OpenAIClient(api_key="sk-test", max_retries=3)
            text, record = client.classify("sys", "user")

        assert text == "ok"
        assert mock_client.chat.completions.create.call_count == 2
        mock_sleep.assert_called_once()


class TestOpenAIClientCostComputation:
    def test_cost_computation_gpt4o(self):
        record = UsageRecord(
            model="gpt-4o",
            input_tokens=1_000_000,
            output_tokens=1_000_000,
        )
        client = OpenAIClient(api_key="sk-test", model="gpt-4o")
        cost = client._compute_cost(record)
        assert cost == 12.50

    def test_cost_computation_gpt4o_mini(self):
        record = UsageRecord(
            model="gpt-4o-mini",
            input_tokens=1_000_000,
            output_tokens=1_000_000,
        )
        client = OpenAIClient(api_key="sk-test", model="gpt-4o-mini")
        cost = client._compute_cost(record)
        assert cost == 0.75

    def test_cost_computation_unknown_model_defaults_to_gpt4o(self):
        record = UsageRecord(
            model="unknown-model",
            input_tokens=1_000_000,
            output_tokens=1_000_000,
        )
        client = OpenAIClient(api_key="sk-test", model="unknown-model")
        cost = client._compute_cost(record)
        assert cost == 12.50

    def test_cost_tracker_appends(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 100
        mock_usage.completion_tokens = 50

        mock_choice = MagicMock()
        mock_choice.message.content = "test"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.return_value = mock_response
        fake_openai_module.OpenAI.return_value = mock_client

        tracker: list[UsageRecord] = []
        client = OpenAIClient(api_key="sk-test", cost_tracker=tracker)
        client.classify("sys", "user")

        assert len(tracker) == 1
        assert tracker[0].input_tokens == 100
        assert tracker[0].output_tokens == 50
        assert tracker[0].cost > 0


class TestOpenAIClientJSONMode:
    def test_supports_json_mode_gpt4(self):
        client = OpenAIClient(api_key="sk-test", model="gpt-4")
        assert client._supports_json_mode()

    def test_supports_json_mode_gpt4o(self):
        client = OpenAIClient(api_key="sk-test", model="gpt-4o")
        assert client._supports_json_mode()

    def test_supports_json_mode_gpt35(self):
        client = OpenAIClient(api_key="sk-test", model="gpt-3.5-turbo")
        assert client._supports_json_mode()

    def test_does_not_support_json_mode_other(self):
        client = OpenAIClient(api_key="sk-test", model="some-other-model")
        assert not client._supports_json_mode()

    def test_json_mode_passed_when_cache_key_and_supported(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 10
        mock_usage.completion_tokens = 5

        mock_choice = MagicMock()
        mock_choice.message.content = '{"result": "ok"}'

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.return_value = mock_response
        fake_openai_module.OpenAI.return_value = mock_client

        client = OpenAIClient(api_key="sk-test", model="gpt-4o")
        client.classify("sys", "user", cache_key="test-key")

        call_kwargs = mock_client.chat.completions.create.call_args[1]
        assert call_kwargs["response_format"] == {"type": "json_object"}

    def test_json_mode_not_passed_when_no_cache_key(self, fake_openai_module):
        mock_usage = MagicMock()
        mock_usage.prompt_tokens = 10
        mock_usage.completion_tokens = 5

        mock_choice = MagicMock()
        mock_choice.message.content = "ok"

        mock_response = MagicMock()
        mock_response.choices = [mock_choice]
        mock_response.usage = mock_usage

        mock_client = MagicMock()
        mock_client.chat.completions.create.return_value = mock_response
        fake_openai_module.OpenAI.return_value = mock_client

        client = OpenAIClient(api_key="sk-test", model="gpt-4o")
        client.classify("sys", "user")

        call_kwargs = mock_client.chat.completions.create.call_args[1]
        assert "response_format" not in call_kwargs
