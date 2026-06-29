from __future__ import annotations

import importlib
import logging
import os
from typing import Any

from symeraseme.core.secrets import SecretResolutionError, resolve_secret
from symeraseme.llm.protocol import LLMClient, LLMProviderError

logger = logging.getLogger(__name__)

# ── Provider registry ────────────────────────────────────────────────
# Each entry maps a provider name to its lazy-load details.
# format: { name: (module_path, class_name, api_key_env_var, default_model) }

_PROVIDERS: dict[str, tuple[str, str, str, str]] = {
    "anthropic": (
        "symeraseme.llm.anthropic_client",
        "AnthropicClient",
        "ANTHROPIC_API_KEY",
        "claude-sonnet-4-6",
    ),
    "openai": (
        "symeraseme.llm.openai_client",
        "OpenAIClient",
        "OPENAI_API_KEY",
        "gpt-4o",
    ),
    "ollama": (
        "symeraseme.llm.ollama_client",
        "OllamaClient",
        "",
        "llama3.1",
    ),
    "openai-compatible": (
        "symeraseme.llm.openai_compatible_client",
        "OpenAICompatibleClient",
        "",
        "default",
    ),
    "agent": (
        "symeraseme.llm.agent_client",
        "AgentLLMClient",
        "",
        "auto",
    ),
}

_ENV_PROVIDER = "SYMERASEME_LLM_PROVIDER"
_ENV_MODEL = "SYMERASEME_LLM_MODEL"
_ENV_BASE_URL = "SYMERASEME_LLM_BASE_URL"
_ENV_OLLAMA_HOST = "OLLAMA_HOST"
_DEFAULT_OLLAMA_HOST = "http://localhost:11434"


def list_available_providers() -> list[str]:
    """Return the names of all known provider backends."""
    return list(_PROVIDERS.keys())


def create_llm_client(
    provider: str | None = None,
    model: str | None = None,
    api_key: str | None = None,
    base_url: str | None = None,
    cost_tracker: list | None = None,
) -> LLMClient:
    """Create an LLM client for the given provider.

    Parameters:
        provider:  Provider name (e.g. "anthropic", "openai-compatible").
                   Falls back to `SYMERASEME_LLM_PROVIDER` env var,
                   then to "anthropic".
        model:     Model name. Falls back to `SYMERASEME_LLM_MODEL`
                   env var, then to the provider's default.
        api_key:   API key. Falls back to the provider's env var.
        base_url:  Custom API base URL. Falls back to
                   `SYMERASEME_LLM_BASE_URL` env var. Only used for
                   providers that support custom endpoints.
        cost_tracker:  Mutable list shared across calls for cost
                   tracking (passed through to the client).

    Returns:
        An `LLMClient`-shaped object for the selected provider.

    Raises:
        LLMProviderError: If the provider is unknown.
    """
    if provider is None:
        provider = os.environ.get(_ENV_PROVIDER, "anthropic")

    try:
        module_path, class_name, key_env, default_model = _PROVIDERS[provider]
    except KeyError:
        known = ", ".join(sorted(_PROVIDERS.keys()))
        raise LLMProviderError(
            f"Unknown LLM provider '{provider}'. Known providers: {known}"
        ) from None

    if model is None:
        env_model = os.environ.get(_ENV_MODEL, "")
        model = env_model if env_model else default_model

    if api_key is None and key_env:
        raw = os.environ.get(key_env, "")
        if raw:
            try:
                api_key = resolve_secret(raw)
            except SecretResolutionError as exc:
                raise LLMProviderError(
                    f"Failed to resolve API key from {key_env}: {exc}. "
                    f"Check your vault:// reference or environment variable."
                ) from exc
        else:
            logger.warning(
                "Environment variable %s is not set. "
                "The LLM client will be created without an API key.",
                key_env,
            )

    if base_url is None:
        base_url = os.environ.get(_ENV_BASE_URL)

    klass = _lazy_import(module_path, class_name)

    kwargs: dict[str, Any] = {
        "model": model,
    }
    if api_key is not None:
        kwargs["api_key"] = api_key
    if cost_tracker is not None:
        kwargs["cost_tracker"] = cost_tracker
    if provider == "ollama":
        host = os.environ.get(_ENV_OLLAMA_HOST, _DEFAULT_OLLAMA_HOST)
        kwargs["host"] = host
    elif provider == "openai-compatible" and base_url is not None:
        kwargs["base_url"] = base_url

    instance: LLMClient = klass(**kwargs)
    return instance


def _lazy_import(module_path: str, class_name: str) -> type:
    """Import a class lazily, raising LLMProviderError on failure."""
    try:
        mod = importlib.import_module(module_path)
    except ImportError as exc:
        raise LLMProviderError(
            f"Cannot import {module_path!r}: {exc}. Is the provider SDK installed?"
        ) from exc
    try:
        return getattr(mod, class_name)
    except AttributeError as exc:
        raise LLMProviderError(
            f"Module {module_path!r} has no class {class_name!r}: {exc}"
        ) from exc
