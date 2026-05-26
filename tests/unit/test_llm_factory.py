from __future__ import annotations

import os
from unittest.mock import patch

import pytest

from symeraseme.llm.factory import create_llm_client, list_available_providers
from symeraseme.llm.protocol import LLMClient, LLMProviderError


class TestListAvailableProviders:
    def test_anthropic_is_available(self):
        providers = list_available_providers()
        assert "anthropic" in providers

    def test_openai_is_available(self):
        providers = list_available_providers()
        assert "openai" in providers

    def test_ollama_is_available(self):
        providers = list_available_providers()
        assert "ollama" in providers

    def test_returns_list_of_strings(self):
        providers = list_available_providers()
        assert isinstance(providers, list)
        assert all(isinstance(p, str) for p in providers)


class TestCreateLLMClientUnknownProvider:
    def test_unknown_provider_raises(self):
        with pytest.raises(LLMProviderError, match="Unknown LLM provider"):
            create_llm_client(provider="nonexistent-xyz")


class TestCreateLLMClientAnthropic:
    def test_creates_anthropic_client_when_sdk_available(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        with patch.dict(os.environ, {}, clear=True):
            client = create_llm_client(provider="anthropic", api_key="sk-test")
            assert isinstance(client, LLMClient)
            assert client.is_available()

    def test_anthropic_client_is_not_available_without_key(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        with patch.dict(os.environ, {}, clear=True):
            client = create_llm_client(provider="anthropic")
            assert not client.is_available()


class TestCreateLLMClientOpenAI:
    def test_creates_openai_client(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-test"}, clear=True):
            client = create_llm_client(provider="openai")
            assert isinstance(client, LLMClient)
            assert hasattr(client, "classify")

    def test_openai_client_is_not_available_without_key(self):
        with patch.dict(os.environ, {}, clear=True):
            client = create_llm_client(provider="openai")
            assert not client.is_available()

    def test_openai_model_defaults_to_gpt4o(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-test"}, clear=True):
            client = create_llm_client(provider="openai")
            assert client.model == "gpt-4o"


class TestCreateLLMClientOllama:
    def test_creates_ollama_client(self):
        client = create_llm_client(provider="ollama")
        assert isinstance(client, LLMClient)
        assert client.model == "llama3.1"

    def test_ollama_host_from_env_var(self):
        with patch.dict(os.environ, {"OLLAMA_HOST": "http://ollama.local:11434"}, clear=True):
            client = create_llm_client(provider="ollama")
            assert client.host == "http://ollama.local:11434"

    def test_ollama_default_host(self):
        with patch.dict(os.environ, {}, clear=True):
            client = create_llm_client(provider="ollama")
            assert client.host == "http://localhost:11434"


class TestEnvVarFallback:
    def test_provider_fallback_to_anthropic(self):
        with patch.dict(os.environ, {}, clear=True):
            client = create_llm_client()
            assert isinstance(client, LLMClient)

    def test_env_var_selects_provider(self):
        with patch.dict(os.environ, {"SYMERASEME_LLM_PROVIDER": "anthropic"}):
            client = create_llm_client(api_key="sk-test-env")
            assert isinstance(client, LLMClient)

    def test_model_env_var_override(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        with patch.dict(os.environ, {"SYMERASEME_LLM_MODEL": "claude-3-5-haiku-latest"}):
            client = create_llm_client(api_key="sk-test")
            assert client.model == "claude-3-5-haiku-latest"

    def test_anthropic_api_key_backward_compat(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "sk-backward-compat"}, clear=True):
            client = create_llm_client()
            assert client.is_available()

    def test_explicit_api_key_takes_priority(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "sk-env"}, clear=True):
            client = create_llm_client(api_key="sk-explicit")
            assert client._api_key == "sk-explicit"


class TestLLMClientInterface:
    def test_returned_client_has_is_available(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        client = create_llm_client(api_key="sk-test")
        assert hasattr(client, "is_available")
        assert callable(client.is_available)

    def test_returned_client_has_classify(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        client = create_llm_client(api_key="sk-test")
        assert hasattr(client, "classify")
        assert callable(client.classify)


class TestCostTrackerPassthrough:
    def test_cost_tracker_is_passed_to_client(self):
        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")
        tracker: list = []
        client = create_llm_client(api_key="sk-test", cost_tracker=tracker)
        assert client.cost_tracker is tracker


class TestLazyImportError:
    def test_missing_module_raises_provider_error(self):
        with patch.dict("symeraseme.llm.factory._PROVIDERS", {
            "broken": ("nonexistent.module.path", "SomeClass", "API_KEY", "model"),
        }), pytest.raises(LLMProviderError, match="Cannot import"):
            create_llm_client(provider="broken")

    def test_missing_class_raises_provider_error(self):
        with patch.dict("symeraseme.llm.factory._PROVIDERS", {
            "badclass": ("symeraseme.llm.protocol", "DoesNotExist", "API_KEY", "model"),
        }), pytest.raises(LLMProviderError, match="has no class"):
            create_llm_client(provider="badclass")
