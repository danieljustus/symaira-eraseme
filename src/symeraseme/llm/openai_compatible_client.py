"""OpenAI-compatible API client for any provider that exposes the OpenAI chat completions API.

Covers: Hermes, Groq, Together, vLLM, LM Studio, OpenRouter, and custom endpoints.
"""

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


class OpenAICompatibleClient(BaseLLMClient):
    """Wrapper around any OpenAI-compatible chat completions API.

    Unlike OpenAIClient, this client accepts a base_url for custom endpoints
    and does not depend on a specific model name for availability checks.
    """

    def __init__(
        self,
        *,
        api_key: str | None = None,
        model: str = "default",
        base_url: str | None = None,
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        super().__init__(model=model, max_retries=max_retries, cost_tracker=cost_tracker)
        self._client: Any = None
        self._api_key = api_key
        self._base_url = base_url

    @property
    def client(self) -> Any:
        if self._client is None:
            import openai

            kwargs: dict[str, Any] = {}
            if self._api_key is not None:
                kwargs["api_key"] = self._api_key
            if self._base_url is not None:
                kwargs["base_url"] = self._base_url
            self._client = openai.OpenAI(**kwargs)
        return self._client

    def is_available(self) -> bool:
        """Check if the provider is reachable and the SDK is importable."""
        try:
            import openai  # noqa: F401
        except ImportError:
            return False
        # For OpenAI-compatible providers, we only need a base_url or an API key.
        # Some local providers (e.g. LM Studio, vLLM) don't require a key.
        return self._base_url is not None or (self._api_key is not None and len(self._api_key) > 0)

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
        """Compute USD cost for a usage record.

        Custom providers often don't have public pricing — default to 0.
        """
        # No pricing data for custom endpoints; cost is 0.
        return 0.0
