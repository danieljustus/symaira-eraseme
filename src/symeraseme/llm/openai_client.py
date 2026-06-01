"""OpenAI API client with cost tracking and retry logic."""

from __future__ import annotations

import logging
import time
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
    "gpt-4o-mini": (0.15, 0.60),
    "gpt-4o": (2.50, 10.00),
    "gpt-4-turbo": (10.00, 30.00),
    "gpt-4": (30.00, 60.00),
    "gpt-3.5-turbo": (0.50, 1.50),
}


class OpenAIClient(BaseLLMClient):
    """Wrapper around the OpenAI SDK with cost tracking and retry."""

    def __init__(
        self,
        *,
        api_key: str | None = None,
        model: str = "gpt-4o",
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        super().__init__(model=model, max_retries=max_retries, cost_tracker=cost_tracker)
        self._client: Any = None
        self._api_key = api_key

    @property
    def client(self) -> Any:
        if self._client is None:
            import openai

            self._client = openai.OpenAI(api_key=self._api_key)
        return self._client

    def is_available(self) -> bool:
        """Check if the OpenAI API is available (key set, SDK importable)."""
        try:
            import openai  # noqa: F401
        except ImportError:
            return False
        if self._api_key is None:
            import os

            self._api_key = os.environ.get("OPENAI_API_KEY")
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
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
        }

        if cache_key and self._supports_json_mode():
            kwargs["response_format"] = {"type": "json_object"}

        import openai

        try:
            response = self.client.chat.completions.create(**kwargs)
        except openai.RateLimitError as e:
            raise LLMClientRateLimitError(str(e)) from e
        except (openai.APIStatusError, openai.APIConnectionError) as e:
            raise LLMClientError(str(e)) from e

        response_text = ""
        if response.choices and len(response.choices) > 0:
            choice = response.choices[0]
            if choice.message and choice.message.content:
                response_text = choice.message.content

        usage = response.usage
        record = UsageRecord(
            model=self.model,
            input_tokens=usage.prompt_tokens if usage else 0,
            output_tokens=usage.completion_tokens if usage else 0,
            cache_creation_tokens=0,
            cache_read_tokens=0,
        )
        record.cost = self._compute_cost(record)
        self.cost_tracker.append(record)

        return response_text.strip(), record

    def _compute_cost(self, record: UsageRecord) -> float:
        """Compute USD cost for a usage record."""
        # Find the closest matching model in pricing table
        pricing = None
        for model_key, prices in MODEL_PRICING.items():
            if model_key in self.model:
                pricing = prices
                break

        if pricing is None:
            # Default to gpt-4o pricing
            pricing = MODEL_PRICING["gpt-4o"]

        input_price_per_m = pricing[0]
        output_price_per_m = pricing[1]

        input_cost = (record.input_tokens / 1_000_000) * input_price_per_m
        output_cost = (record.output_tokens / 1_000_000) * output_price_per_m

        return round(input_cost + output_cost, 6)

    def _supports_json_mode(self) -> bool:
        """Check if the selected model supports JSON mode."""
        return "gpt-4" in self.model or "gpt-3.5-turbo" in self.model
