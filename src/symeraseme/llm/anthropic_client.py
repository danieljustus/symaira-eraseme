"""Anthropic Claude API client with cost tracking and retry logic."""

from __future__ import annotations

import logging
import time  # noqa: F401
from typing import Any

from symeraseme.llm.protocol import (
    BaseLLMClient,
    LLMClientError,
    LLMClientRateLimitError,
    UsageRecord,
)

logger = logging.getLogger(__name__)

# Known model pricing per 1M input tokens and 1M output tokens (USD)
# As of May 2026
MODEL_PRICING: dict[str, tuple[float, float]] = {
    "claude-3-5-sonnet-latest": (3.00, 15.00),
    "claude-3-5-haiku-latest": (1.00, 5.00),
    "claude-3-opus-latest": (15.00, 75.00),
}


class AnthropicClient(BaseLLMClient):
    """Wrapper around the Anthropic SDK with prompt caching and retry."""

    def __init__(
        self,
        *,
        api_key: str | None = None,
        model: str = "claude-3-5-sonnet-latest",
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        super().__init__(model=model, max_retries=max_retries, cost_tracker=cost_tracker)
        self._client: Any = None
        self._api_key = api_key

    @property
    def client(self) -> Any:
        if self._client is None:
            import anthropic

            self._client = anthropic.Anthropic(api_key=self._api_key)
        return self._client

    def is_available(self) -> bool:
        """Check if the Anthropic API is available (key set, SDK importable)."""
        try:
            import anthropic  # noqa: F401
        except ImportError:
            return False
        if self._api_key is None:
            import os

            self._api_key = os.environ.get("ANTHROPIC_API_KEY")
        return self._api_key is not None and len(self._api_key) > 0

    def _call_api(
        self,
        *,
        system_prompt: str,
        user_prompt: str,
        max_tokens: int,
        temperature: float,
        cache_key: str | None,
    ) -> tuple[str, UsageRecord]:
        kwargs: dict[str, Any] = {
            "model": self.model,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "system": [{"type": "text", "text": system_prompt}],
            "messages": [{"role": "user", "content": user_prompt}],
        }

        if cache_key and self._supports_prompt_caching():
            # Mark the system prompt as ephemeral for caching
            kwargs["system"] = [
                {
                    "type": "text",
                    "text": system_prompt,
                    "cache_control": {"type": "ephemeral"},
                }
            ]

        import anthropic

        try:
            message = self.client.messages.create(**kwargs)
        except anthropic.RateLimitError as e:
            raise AnthropicClientRateLimitError(str(e)) from e
        except (anthropic.APIStatusError, anthropic.APIConnectionError) as e:
            raise AnthropicClientError(str(e)) from e

        response_text = ""
        for block in message.content:
            if hasattr(block, "text"):
                response_text += block.text

        usage = message.usage
        record = UsageRecord(
            model=self.model,
            input_tokens=usage.input_tokens or 0,
            output_tokens=usage.output_tokens or 0,
            cache_creation_tokens=getattr(usage, "cache_creation_input_tokens", 0) or 0,
            cache_read_tokens=getattr(usage, "cache_read_input_tokens", 0) or 0,
        )
        record.cost = self._compute_cost(record)
        self.cost_tracker.append(record)

        return response_text.strip(), record

    def _compute_cost(self, record: UsageRecord) -> float:
        """Compute USD cost for a usage record."""
        pricing = MODEL_PRICING.get(self.model, (3.0, 15.0))
        input_price_per_m = pricing[0]
        output_price_per_m = pricing[1]

        input_cost = (record.input_tokens / 1_000_000) * input_price_per_m
        output_cost = (record.output_tokens / 1_000_000) * output_price_per_m

        # Cached reads are ~50% cheaper (approximate)
        cache_read_savings = (record.cache_read_tokens / 1_000_000) * input_price_per_m * 0.5
        return round(input_cost + output_cost - cache_read_savings, 6)

    def _supports_prompt_caching(self) -> bool:
        """Check if the selected model supports prompt caching."""
        return "sonnet" in self.model or "haiku" in self.model


class AnthropicClientError(LLMClientError):
    pass


class AnthropicClientRateLimitError(LLMClientRateLimitError):
    pass
