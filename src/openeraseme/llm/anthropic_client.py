"""Anthropic Claude API client with cost tracking and retry logic."""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from typing import Any

logger = logging.getLogger(__name__)

# Known model pricing per 1M input tokens and 1M output tokens (USD)
# As of May 2026
MODEL_PRICING: dict[str, tuple[float, float]] = {
    "claude-3-5-sonnet-latest": (3.00, 15.00),
    "claude-3-5-haiku-latest": (1.00, 5.00),
    "claude-3-opus-latest": (15.00, 75.00),
}


@dataclass
class UsageRecord:
    model: str
    input_tokens: int = 0
    output_tokens: int = 0
    cache_creation_tokens: int = 0
    cache_read_tokens: int = 0
    cost: float = 0.0

    def record(self) -> dict[str, Any]:
        return {
            "model": self.model,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_creation_tokens": self.cache_creation_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "cost": self.cost,
        }


class AnthropicClient:
    """Wrapper around the Anthropic SDK with prompt caching and retry."""

    def __init__(
        self,
        *,
        api_key: str | None = None,
        model: str = "claude-3-5-sonnet-latest",
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        self.model = model
        self.max_retries = max_retries
        self.cost_tracker = cost_tracker if cost_tracker is not None else []
        self._client: Any = None

        # Lazy import to avoid hard dependency at module level
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

    def classify(
        self,
        system_prompt: str,
        user_prompt: str,
        *,
        max_tokens: int = 512,
        temperature: float = 0.0,
        cache_key: str | None = None,
    ) -> tuple[str, UsageRecord]:
        """Send a classification request to Claude.

        Returns (response_text, usage_record).
        Throws AnthropicError on failure.
        """
        if not self.is_available():
            msg = "Anthropic API is not available (no API key or SDK not installed)"
            raise AnthropicClientError(msg)

        last_error: Exception | None = None
        for attempt in range(1, self.max_retries + 1):
            try:
                return self._call_api(
                    system_prompt=system_prompt,
                    user_prompt=user_prompt,
                    max_tokens=max_tokens,
                    temperature=temperature,
                    cache_key=cache_key,
                )
            except AnthropicClientRateLimitError as e:
                last_error = e
                if attempt < self.max_retries:
                    wait = 2**attempt + (hash(str(cache_key)) % 5)
                    logger.warning(
                        "Rate limited (attempt %d/%d), retrying in %ds",
                        attempt,
                        self.max_retries,
                        wait,
                    )
                    time.sleep(wait)
            except AnthropicClientError as e:
                last_error = e
                if attempt < self.max_retries:
                    wait = 2**attempt
                    logger.warning(
                        "API error (attempt %d/%d): %s. Retrying in %ds",
                        attempt,
                        self.max_retries,
                        e,
                        wait,
                    )
                    time.sleep(wait)
                else:
                    raise

        msg = f"All {self.max_retries} retries exhausted: {last_error}"
        raise AnthropicClientError(msg) from last_error

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

        message = self.client.messages.create(**kwargs)

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


class AnthropicClientError(Exception):
    pass


class AnthropicClientRateLimitError(AnthropicClientError):
    pass
