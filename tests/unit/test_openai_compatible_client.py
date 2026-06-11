from __future__ import annotations

import inspect
from unittest.mock import patch

from symeraseme.llm.protocol import LLMClient


class TestOpenAICompatibleClientStructuralSubtyping:
    def test_is_instance_of_llm_client_protocol(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(base_url="http://localhost:8080/v1")
        assert isinstance(client, LLMClient)

    def test_has_is_available_method(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(base_url="http://localhost:8080/v1")
        assert hasattr(client, "is_available")
        assert callable(client.is_available)

    def test_has_classify_method(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(base_url="http://localhost:8080/v1")
        assert hasattr(client, "classify")
        assert callable(client.classify)


class TestOpenAICompatibleClientAvailability:
    def test_available_with_base_url(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(base_url="http://localhost:8080/v1")
        assert client.is_available()

    def test_available_with_api_key_only(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(api_key="sk-test")
        assert client.is_available()

    def test_not_available_without_key_or_url(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient()
        assert not client.is_available()

    def test_not_available_without_openai_sdk(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = OpenAICompatibleClient(base_url="http://localhost:8080/v1")
        with patch.dict("sys.modules", {"openai": None}):
            assert not client.is_available()


class TestOpenAICompatibleClientFactoryIntegration:
    def test_factory_creates_openai_compatible_client(self):
        from symeraseme.llm.factory import create_llm_client
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient

        client = create_llm_client(
            provider="openai-compatible",
            base_url="http://localhost:8080/v1",
        )
        assert isinstance(client, OpenAICompatibleClient)
        assert isinstance(client, LLMClient)

    def test_factory_passes_base_url_to_client(self):
        from symeraseme.llm.factory import create_llm_client

        client = create_llm_client(
            provider="openai-compatible",
            base_url="http://custom-host:9090/v1",
        )
        assert client._base_url == "http://custom-host:9090/v1"

    def test_factory_uses_env_var_for_base_url(self):
        from symeraseme.llm.factory import create_llm_client

        with patch.dict(
            "os.environ",
            {"SYMERASEME_LLM_BASE_URL": "http://env-host:8080/v1"},
        ):
            client = create_llm_client(provider="openai-compatible")
            assert client._base_url == "http://env-host:8080/v1"

    def test_factory_model_override(self):
        from symeraseme.llm.factory import create_llm_client

        client = create_llm_client(
            provider="openai-compatible",
            base_url="http://localhost:8080/v1",
            model="hermes-3",
        )
        assert client.model == "hermes-3"

    def test_factory_uses_env_var_for_model(self):
        from symeraseme.llm.factory import create_llm_client

        with patch.dict(
            "os.environ",
            {"SYMERASEME_LLM_MODEL": "llama-3.1-8b"},
        ):
            client = create_llm_client(
                provider="openai-compatible",
                base_url="http://localhost:8080/v1",
            )
            assert client.model == "llama-3.1-8b"


class TestOpenAICompatibleClientSignature:
    def test_classify_signature_matches_protocol(self):
        from symeraseme.llm.openai_compatible_client import OpenAICompatibleClient
        from symeraseme.llm.protocol import LLMClient

        proto_sig = inspect.signature(LLMClient.classify)
        impl_sig = inspect.signature(OpenAICompatibleClient.classify)

        proto_params = list(proto_sig.parameters.keys())
        impl_params = list(impl_sig.parameters.keys())
        assert proto_params == impl_params, (
            f"Parameter mismatch: protocol={proto_params}, implementation={impl_params}"
        )

        assert impl_sig.return_annotation == proto_sig.return_annotation, (
            f"Return annotation mismatch: protocol={proto_sig.return_annotation}, "
            f"implementation={impl_sig.return_annotation}"
        )


class TestProviderRegistryIncludesOpenAICompatible:
    def test_openai_compatible_in_providers(self):
        from symeraseme.llm.factory import list_available_providers

        providers = list_available_providers()
        assert "openai-compatible" in providers

    def test_all_expected_providers_registered(self):
        from symeraseme.llm.factory import list_available_providers

        providers = list_available_providers()
        expected = {"anthropic", "openai", "ollama", "openai-compatible"}
        assert expected.issubset(set(providers))
