"""Ollama local LLM client using only stdlib — no external HTTP dependency."""

from __future__ import annotations

import json
import logging
import urllib.error
import urllib.request

from symeraseme.llm.protocol import BaseLLMClient, LLMClientError, UsageRecord

logger = logging.getLogger(__name__)

_DEFAULT_TIMEOUT = 30  # seconds


class OllamaClient(BaseLLMClient):
    """Wrapper around a local Ollama instance via urllib.request."""

    def __init__(
        self,
        *,
        host: str = "http://localhost:11434",
        model: str = "llama3.1",
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        super().__init__(model=model, max_retries=max_retries, cost_tracker=cost_tracker)
        self.host = host.rstrip("/")

    def is_available(self) -> bool:
        """Check if the Ollama server is reachable and the model is available."""
        try:
            req = urllib.request.Request(
                f"{self.host}/api/tags",
                method="GET",
                headers={"Content-Type": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=_DEFAULT_TIMEOUT) as resp:
                if resp.status != 200:
                    return False
                data = json.loads(resp.read().decode("utf-8"))
                models = data.get("models", [])
                return any(
                    m.get("name") == self.model or m.get("model") == self.model for m in models
                )
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
            return False

    def _call_api(
        self,
        *,
        system_prompt: str,
        user_prompt: str,
        max_tokens: int,
        temperature: float,
        cache_key: str | None,
    ) -> tuple[str, UsageRecord]:
        """Send a classification request to the local Ollama server.

        Returns (response_text, usage_record).
        Raises LLMClientError on failure.
        """
        payload = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "stream": False,
            "options": {
                "temperature": temperature,
                "num_predict": max_tokens,
            },
        }

        req = urllib.request.Request(
            f"{self.host}/api/chat",
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=_DEFAULT_TIMEOUT) as resp:
                if resp.status != 200:
                    raise LLMClientError(f"Ollama returned status {resp.status}")
                data = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise LLMClientError(f"Ollama HTTP error {e.code}: {e.reason}") from e
        except (urllib.error.URLError, TimeoutError) as e:
            raise LLMClientError(f"Ollama connection error: {e}") from e
        except json.JSONDecodeError as e:
            raise LLMClientError(f"Invalid JSON from Ollama: {e}") from e

        response_text = ""
        message = data.get("message", {})
        if message:
            response_text = message.get("content", "")

        # Ollama provides prompt_eval_count and eval_count for token usage
        prompt_tokens = data.get("prompt_eval_count", 0) or 0
        eval_tokens = data.get("eval_count", 0) or 0

        record = UsageRecord(
            model=self.model,
            input_tokens=prompt_tokens,
            output_tokens=eval_tokens,
            cache_creation_tokens=0,
            cache_read_tokens=0,
            cost=0.0,
        )
        self.cost_tracker.append(record)

        return response_text.strip(), record

    def _compute_cost(self, record: UsageRecord) -> float:
        return 0.0
