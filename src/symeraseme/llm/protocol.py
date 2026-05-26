"""Generic LLM client protocol — structural typing, no hard deps on any vendor SDK."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Protocol, runtime_checkable


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
