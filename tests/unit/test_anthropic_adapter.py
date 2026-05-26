from __future__ import annotations

import inspect

import pytest

from openeraseme.llm.protocol import (
    LLMClient,
    LLMClientError,
    LLMClientRateLimitError,
)
from openeraseme.llm.protocol import (
    UsageRecord as ProtocolUsageRecord,
)


class TestAnthropicClientStructuralSubtyping:
    def test_is_instance_of_llm_client_protocol(self):
        from openeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert isinstance(client, LLMClient)

    def test_has_is_available_method(self):
        from openeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert hasattr(client, "is_available")
        assert callable(client.is_available)

    def test_has_classify_method(self):
        from openeraseme.llm.anthropic_client import AnthropicClient

        client = AnthropicClient(api_key="sk-test")
        assert hasattr(client, "classify")
        assert callable(client.classify)


class TestAnthropicClientExceptionHierarchy:
    def test_anthropic_client_error_inherits_from_llm_client_error(self):
        from openeraseme.llm.anthropic_client import AnthropicClientError

        err = AnthropicClientError("boom")
        assert isinstance(err, LLMClientError)

    def test_anthropic_rate_limit_error_inherits_from_llm_rate_limit_error(self):
        from openeraseme.llm.anthropic_client import AnthropicClientRateLimitError

        err = AnthropicClientRateLimitError("rate limited")
        assert isinstance(err, LLMClientRateLimitError)
        assert isinstance(err, LLMClientError)

    def test_generic_catches_anthropic_error(self):
        from openeraseme.llm.anthropic_client import AnthropicClientError

        with pytest.raises(LLMClientError):
            raise AnthropicClientError("test")

    def test_generic_rate_limit_catches_anthropic_rate_limit(self):
        from openeraseme.llm.anthropic_client import AnthropicClientRateLimitError

        with pytest.raises(LLMClientRateLimitError):
            raise AnthropicClientRateLimitError("test")
        with pytest.raises(LLMClientError):
            raise AnthropicClientRateLimitError("test")


class TestUsageRecordBackwardCompatibility:
    def test_usage_record_is_same_class_as_protocol(self):
        from openeraseme.llm.anthropic_client import UsageRecord as AnthropicUsageRecord

        assert AnthropicUsageRecord is ProtocolUsageRecord

    def test_usage_record_can_be_instantiated_via_anthropic_import(self):
        from openeraseme.llm.anthropic_client import UsageRecord

        rec = UsageRecord(
            model="claude-3-5-sonnet-latest",
            input_tokens=100,
            output_tokens=50,
            cost=0.0042,
        )
        assert rec.model == "claude-3-5-sonnet-latest"
        assert rec.input_tokens == 100
        assert rec.output_tokens == 50
        assert rec.cost == 0.0042

    def test_usage_record_record_method_works_via_anthropic_import(self):
        from openeraseme.llm.anthropic_client import UsageRecord

        rec = UsageRecord(model="test", input_tokens=10, output_tokens=5, cost=1.0)
        result = rec.record()
        assert result["model"] == "test"
        assert result["input_tokens"] == 10
        assert result["output_tokens"] == 5
        assert result["cost"] == 1.0


class TestFactoryProducesAnthropicClient:
    def test_factory_creates_anthropic_client(self):
        from openeraseme.llm.anthropic_client import AnthropicClient
        from openeraseme.llm.factory import create_llm_client

        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")

        client = create_llm_client(provider="anthropic", api_key="sk-test")
        assert isinstance(client, AnthropicClient)
        assert isinstance(client, LLMClient)

    def test_factory_anthropic_client_is_available_with_key(self):
        from openeraseme.llm.factory import create_llm_client

        try:
            import anthropic  # noqa: F401
        except ImportError:
            pytest.skip("anthropic SDK not installed")

        client = create_llm_client(provider="anthropic", api_key="sk-test")
        assert client.is_available()

    def test_factory_anthropic_client_classify_signature_matches(self):
        from openeraseme.llm.anthropic_client import AnthropicClient
        from openeraseme.llm.protocol import LLMClient

        proto_sig = inspect.signature(LLMClient.classify)
        impl_sig = inspect.signature(AnthropicClient.classify)

        proto_params = list(proto_sig.parameters.keys())
        impl_params = list(impl_sig.parameters.keys())
        assert proto_params == impl_params, (
            f"Parameter mismatch: protocol={proto_params}, implementation={impl_params}"
        )

        assert impl_sig.return_annotation == proto_sig.return_annotation, (
            f"Return annotation mismatch: protocol={proto_sig.return_annotation}, "
            f"implementation={impl_sig.return_annotation}"
        )
