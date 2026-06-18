"""Generic LLM client protocol — structural typing, no hard deps on any vendor SDK."""

from __future__ import annotations

import logging
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, Protocol, runtime_checkable

logger = logging.getLogger(__name__)


@dataclass
class UsageRecord:
    """Generic LLM usage and cost record.

    Mirrors the signature from anthropic_client.py exactly for
    backward compatibility — same fields, same `record()` method.
    """

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


class LLMClientError(Exception):
    """Generic error raised by any LLM provider client."""


class LLMClientRateLimitError(LLMClientError):
    """Rate-limit error — callers can inspect and retry."""


class LLMProviderError(LLMClientError):
    """Raised when an unknown or unavailable provider is requested."""


@runtime_checkable
class LLMClient(Protocol):
    """Structural interface every provider-specific client must satisfy.

    No explicit inheritance required — any object with these methods
    at runtime is a valid `LLMClient`.

    Required:
        is_available  — can the client make API calls right now?
        classify      — send a classification prompt, return (text, usage)

    Optional (callers should use hasattr before invoking):
        close         — release resources (connections, sessions, etc.)
    """

    def is_available(self) -> bool:
        """Return True if the client is ready to make API calls."""
        ...

    def classify(
        self,
        system_prompt: str,
        user_prompt: str,
        *,
        max_tokens: int = 512,
        temperature: float = 0.0,
        cache_key: str | None = None,
    ) -> tuple[str, UsageRecord]:
        """Send a classification request and return (response_text, usage).

        Raises:
            LLMClientError: on any provider error.
            LLMClientRateLimitError: when the provider rate-limits the call.
        """
        ...


class BaseLLMClient(ABC):
    """Abstract base class for LLM clients with shared retry, cost tracking,
    and rate-limit handling.

    Subclasses must implement:
        - is_available()
        - _call_api()

    The classify() method provides a unified retry loop with exponential backoff.
    """

    def __init__(
        self,
        *,
        model: str,
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        self.model = model
        self.max_retries = max_retries
        self.cost_tracker = cost_tracker if cost_tracker is not None else []

    @abstractmethod
    def is_available(self) -> bool:
        """Return True if the client is ready to make API calls."""
        ...

    @abstractmethod
    def _call_api(
        self,
        *,
        system_prompt: str,
        user_prompt: str,
        max_tokens: int,
        temperature: float,
        cache_key: str | None,
    ) -> tuple[str, UsageRecord]:
        """Make the provider-specific API call.

        Must raise LLMClientRateLimitError for rate-limit errors
        and LLMClientError for other API errors.
        """
        ...

    def classify(
        self,
        system_prompt: str,
        user_prompt: str,
        *,
        max_tokens: int = 512,
        temperature: float = 0.0,
        cache_key: str | None = None,
    ) -> tuple[str, UsageRecord]:
        """Send a classification request with retry logic.

        Returns (response_text, usage_record).
        Raises LLMClientError on failure.
        """
        if not self.is_available():
            raise LLMClientError(
                f"{self.__class__.__name__} is not available (no API key or SDK not installed)"
            )

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
            except LLMClientRateLimitError as e:
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
                else:
                    raise
            except LLMClientError as e:
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
        raise LLMClientError(msg) from last_error


class OpenAIBaseMixin:
    """Shared _call_api implementation for OpenAI SDK-based clients.

    Provides the common kwargs construction, error handling, and response
    parsing used by both OpenAIClient and OpenAICompatibleClient.

    Subclasses must define: model, client, cost_tracker, _compute_cost().
    """

    model: str
    client: Any
    cost_tracker: list[UsageRecord]

    def _compute_cost(self, record: UsageRecord) -> float:  # pragma: no cover
        raise NotImplementedError

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

    def _supports_json_mode(self) -> bool:
        return False
