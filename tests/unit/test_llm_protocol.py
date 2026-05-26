from __future__ import annotations

import pytest

from symeraseme.llm.protocol import (
    LLMClientError,
    LLMClientRateLimitError,
    LLMProviderError,
    UsageRecord,
)


class TestUsageRecord:
    def test_record_returns_dict_with_all_fields(self):
        rec = UsageRecord(
            model="test-model",
            input_tokens=100,
            output_tokens=50,
            cache_creation_tokens=10,
            cache_read_tokens=5,
            cost=0.0042,
        )
        result = rec.record()
        assert result == {
            "model": "test-model",
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_tokens": 10,
            "cache_read_tokens": 5,
            "cost": 0.0042,
        }

    def test_record_defaults(self):
        rec = UsageRecord(model="default-model")
        result = rec.record()
        assert result["model"] == "default-model"
        assert result["input_tokens"] == 0
        assert result["output_tokens"] == 0
        assert result["cache_creation_tokens"] == 0
        assert result["cache_read_tokens"] == 0
        assert result["cost"] == 0.0

    def test_record_types(self):
        rec = UsageRecord(model="x", input_tokens=1, output_tokens=2, cost=3.0)
        result = rec.record()
        assert isinstance(result, dict)
        assert isinstance(result["model"], str)
        assert isinstance(result["input_tokens"], int)
        assert isinstance(result["output_tokens"], int)
        assert isinstance(result["cost"], float)

    def test_zero_cost_record(self):
        rec = UsageRecord(model="free-model", cost=0.0)
        assert rec.cost == 0.0
        assert rec.record()["cost"] == 0.0

    def test_fields_are_independent(self):
        rec1 = UsageRecord(model="a", input_tokens=1)
        rec2 = UsageRecord(model="b", input_tokens=2)
        rec1.input_tokens = 999
        assert rec2.input_tokens == 2


class TestLLMClientError:
    def test_basic_instantiation(self):
        err = LLMClientError("something went wrong")
        assert isinstance(err, Exception)
        assert str(err) == "something went wrong"

    def test_is_base_for_rate_limit(self):
        err = LLMClientRateLimitError("rate limited")
        assert isinstance(err, LLMClientError)
        assert isinstance(err, Exception)

    def test_rate_limit_is_catchable(self):
        with pytest.raises(LLMClientRateLimitError):
            raise LLMClientRateLimitError("throttled")

    def test_base_error_catches_rate_limit(self):
        with pytest.raises(LLMClientError):
            raise LLMClientRateLimitError("throttled")

    def test_provider_error_is_llm_client_error(self):
        err = LLMProviderError("unknown provider 'x'")
        assert isinstance(err, LLMClientError)
        assert isinstance(err, Exception)


class TestLLMClientProtocol:
    def test_structural_subtyping_with_minimal_impl(self):
        class FakeClient:
            def is_available(self) -> bool:
                return True

            def classify(self, system_prompt, user_prompt, *, max_tokens=512,
                         temperature=0.0, cache_key=None):
                return ("response", UsageRecord(model="fake"))

        client = FakeClient()
        assert client.is_available()
        text, usage = client.classify("sys", "user")
        assert text == "response"
        assert usage.model == "fake"

    def test_optional_close_not_required(self):
        class NoCloseClient:
            def is_available(self) -> bool:
                return False

            def classify(self, system_prompt, user_prompt, *, max_tokens=512,
                         temperature=0.0, cache_key=None):
                return ("", UsageRecord(model="nc"))

        # Should not raise — close is optional
        client = NoCloseClient()
        assert not client.is_available()

    def test_close_when_present(self):
        closed = []

        class ClosableClient:
            def is_available(self) -> bool:
                return True

            def classify(self, system_prompt, user_prompt, *, max_tokens=512,
                         temperature=0.0, cache_key=None):
                return ("ok", UsageRecord(model="cc"))

            def close(self) -> None:
                closed.append(True)

        client = ClosableClient()
        client.close()
        assert closed == [True]
