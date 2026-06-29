"""Host agent LLM client — delegates LLM calls to the host coding agent.

Supports Claude Code, Hermes, and GitHub Copilot as LLM backends,
allowing symeraseme to use the host agent's model instead of requiring
separate API keys or a local Ollama instance.

Activation::

    export SYMERASEME_LLM_PROVIDER=agent
    # Optional: force a specific backend
    export SYMERASEME_AGENT_BACKEND=claude
"""

from __future__ import annotations

import logging
import os
import shutil
import subprocess
from dataclasses import dataclass

from symeraseme.llm.protocol import (
    BaseLLMClient,
    LLMClientError,
    UsageRecord,
)

logger = logging.getLogger(__name__)

_ENV_AGENT_BACKEND = "SYMERASEME_AGENT_BACKEND"
_SUBPROCESS_TIMEOUT = 120  # seconds


@dataclass(frozen=True)
class _AgentDef:
    """Definition of a host agent CLI."""

    name: str
    cli: str
    check_cmd: list[str]
    invoke_template: list[str]
    description: str


_AGENT_DEFS: dict[str, _AgentDef] = {
    "claude": _AgentDef(
        name="claude",
        cli="claude",
        check_cmd=["claude", "--version"],
        invoke_template=[
            "claude",
            "-p",
            "{prompt}",
            "--output-format",
            "text",
            "--no-input",
        ],
        description="Claude Code CLI",
    ),
    "hermes": _AgentDef(
        name="hermes",
        cli="hermes",
        check_cmd=["hermes", "--version"],
        invoke_template=["hermes", "-p", "{prompt}"],
        description="Hermes CLI",
    ),
    "copilot": _AgentDef(
        name="copilot",
        cli="gh",
        check_cmd=["gh", "copilot", "--version"],
        invoke_template=["gh", "copilot", "suggest", "-p", "{prompt}"],
        description="GitHub Copilot CLI",
    ),
}


def _is_cli_available(cli: str) -> bool:
    """Check if a CLI binary is on PATH."""
    return shutil.which(cli) is not None


def _detect_agent_backend(explicit: str = "") -> str | None:
    """Detect which host agent CLI is available.

    If ``explicit`` is set, only that backend is checked.  Otherwise
    the function probes Claude Code, Hermes, and GitHub Copilot in
    order of preference.
    """
    if explicit:
        key = explicit.strip().lower()
        defn = _AGENT_DEFS.get(key)
        if defn is not None and _is_cli_available(defn.cli):
            return key
        logger.warning(
            "Explicit agent backend %r requested but CLI %r not found on PATH",
            key,
            defn.cli if defn else key,
        )
        return None

    # Auto-detect: Claude Code > Hermes > Copilot
    for key in ("claude", "hermes", "copilot"):
        if _is_cli_available(_AGENT_DEFS[key].cli):
            return key

    return None


class AgentLLMClient(BaseLLMClient):
    """LLM client that delegates to the host coding agent via subprocess.

    When ``SYMERASEME_LLM_PROVIDER=agent`` this client is used instead of
    direct API clients.  It detects the available host agent (Claude Code,
    Hermes, or GitHub Copilot) and invokes it via subprocess.

    Configuration:
        ``SYMERASEME_AGENT_BACKEND`` — Force a specific backend
            (``claude``, ``hermes``, ``copilot``).  When unset the client
            auto-detects the first available CLI.
    """

    def __init__(
        self,
        *,
        model: str = "auto",
        agent_backend: str | None = None,
        max_retries: int = 3,
        cost_tracker: list[UsageRecord] | None = None,
    ) -> None:
        super().__init__(
            model=model,
            max_retries=max_retries,
            cost_tracker=cost_tracker,
        )
        self._requested_backend = agent_backend or os.environ.get(_ENV_AGENT_BACKEND, "")
        self._resolved_backend: str | None = None
        self._availability_checked = False
        self._is_available = False

    # -- internal helpers ---------------------------------------------------

    def _resolve_backend(self) -> str | None:
        """Resolve and cache the agent backend name."""
        if self._resolved_backend is None:
            self._resolved_backend = _detect_agent_backend(self._requested_backend)
        return self._resolved_backend

    def _build_combined_prompt(self, system_prompt: str, user_prompt: str) -> str:
        """Merge system and user prompts into a single prompt string."""
        parts = [system_prompt, "", "---", "", user_prompt]
        return "\n".join(parts)

    # -- LLMClient protocol ------------------------------------------------

    def is_available(self) -> bool:
        """Return True if a host agent CLI is reachable."""
        if self._availability_checked:
            return self._is_available
        self._availability_checked = True
        self._is_available = self._resolve_backend() is not None
        if self._is_available:
            logger.info("Host agent backend detected: %s", self._resolved_backend)
        else:
            logger.debug("No host agent CLI detected on PATH")
        return self._is_available

    def _call_api(
        self,
        *,
        system_prompt: str,
        user_prompt: str,
        max_tokens: int,
        temperature: float,
        cache_key: str | None,
    ) -> tuple[str, UsageRecord]:
        backend_key = self._resolve_backend()
        if backend_key is None:
            raise LLMClientError(
                "No host agent CLI detected.  Install Claude Code, Hermes, "
                "or GitHub Copilot CLI, or set SYMERASEME_AGENT_BACKEND."
            )

        defn = _AGENT_DEFS[backend_key]
        combined = self._build_combined_prompt(system_prompt, user_prompt)

        cmd: list[str] = []
        for part in defn.invoke_template:
            cmd.append(part.replace("{prompt}", combined))

        # Forward model choice when the backend supports it.
        if self.model and self.model != "auto" and backend_key == "claude":
            cmd.extend(["--model", self.model])

        logger.debug(
            "Invoking host agent %s (first 3 argv: %s)",
            defn.name,
            cmd[:3],
        )

        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=_SUBPROCESS_TIMEOUT,
                env={**os.environ, "TERM": "dumb"},
            )
        except FileNotFoundError as exc:
            raise LLMClientError(
                f"Agent CLI not found: {exc}.  Ensure the host agent is installed and on PATH."
            ) from exc
        except subprocess.TimeoutExpired as exc:
            raise LLMClientError(f"Host agent timed out after {_SUBPROCESS_TIMEOUT}s.") from exc
        except OSError as exc:
            raise LLMClientError(f"Failed to invoke host agent: {exc}") from exc

        if result.returncode != 0:
            stderr = (result.stderr or "").strip()[:500]
            raise LLMClientError(f"Host agent exited with code {result.returncode}: {stderr}")

        response_text = (result.stdout or "").strip()
        if not response_text:
            raise LLMClientError("Host agent returned empty response")

        # No cost data available from host agent subprocess calls.
        record = UsageRecord(
            model=f"agent:{defn.name}",
            input_tokens=0,
            output_tokens=0,
            cache_creation_tokens=0,
            cache_read_tokens=0,
            cost=0.0,
        )
        self.cost_tracker.append(record)

        return response_text, record
