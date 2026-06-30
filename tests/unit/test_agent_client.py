"""Tests for symeraseme.llm.agent_client — host agent LLM client."""

from __future__ import annotations

import subprocess
from unittest.mock import MagicMock, patch

import pytest

from symeraseme.llm.agent_client import (
    AgentLLMClient,
    _detect_agent_backend,
    _is_cli_available,
)
from symeraseme.llm.protocol import LLMClientError, UsageRecord


# ── _is_cli_available ──────────────────────────────────────────────


class TestIsCliAvailable:
    @patch("symeraseme.llm.agent_client.shutil.which", return_value="/usr/bin/claude")
    def test_cli_found(self, mock_which: MagicMock) -> None:
        assert _is_cli_available("claude") is True
        mock_which.assert_called_once_with("claude")

    @patch("symeraseme.llm.agent_client.shutil.which", return_value=None)
    def test_cli_not_found(self, mock_which: MagicMock) -> None:
        assert _is_cli_available("nonexistent") is False


# ── _detect_agent_backend ──────────────────────────────────────────


class TestDetectAgentBackend:
    @patch("symeraseme.llm.agent_client._is_cli_available", return_value=True)
    def test_explicit_valid(self, mock_avail: MagicMock) -> None:
        result = _detect_agent_backend("claude")
        assert result == "claude"

    @patch("symeraseme.llm.agent_client._is_cli_available", return_value=False)
    def test_explicit_invalid_cli(self, mock_avail: MagicMock) -> None:
        result = _detect_agent_backend("claude")
        assert result is None

    def test_explicit_unknown_key(self) -> None:
        result = _detect_agent_backend("nonexistent")
        assert result is None

    @patch("symeraseme.llm.agent_client._is_cli_available")
    def test_auto_detect_claude(self, mock_avail: MagicMock) -> None:
        mock_avail.side_effect = lambda cli: cli == "claude"
        result = _detect_agent_backend("")
        assert result == "claude"

    @patch("symeraseme.llm.agent_client._is_cli_available")
    def test_auto_detect_hermes(self, mock_avail: MagicMock) -> None:
        mock_avail.side_effect = lambda cli: cli == "hermes"
        result = _detect_agent_backend("")
        assert result == "hermes"

    @patch("symeraseme.llm.agent_client._is_cli_available")
    def test_auto_detect_copilot(self, mock_avail: MagicMock) -> None:
        # copilot's CLI is "gh", not "copilot"
        mock_avail.side_effect = lambda cli: cli == "gh"
        result = _detect_agent_backend("")
        assert result == "copilot"

    @patch("symeraseme.llm.agent_client._is_cli_available", return_value=False)
    def test_auto_detect_none_available(self, mock_avail: MagicMock) -> None:
        result = _detect_agent_backend("")
        assert result is None


# ── AgentLLMClient.__init__ ────────────────────────────────────────


class TestAgentLLMClientInit:
    def test_defaults(self) -> None:
        client = AgentLLMClient()
        assert client.model == "auto"
        assert client.max_retries == 3
        assert client._resolved_backend is None
        assert client._availability_checked is False

    def test_custom_model(self) -> None:
        client = AgentLLMClient(model="opus")
        assert client.model == "opus"

    def test_explicit_backend(self) -> None:
        client = AgentLLMClient(agent_backend="hermes")
        assert client._requested_backend == "hermes"

    @patch.dict("os.environ", {"SYMERASEME_AGENT_BACKEND": "copilot"})
    def test_env_backend(self) -> None:
        client = AgentLLMClient()
        assert client._requested_backend == "copilot"

    def test_cost_tracker(self) -> None:
        tracker: list[UsageRecord] = []
        client = AgentLLMClient(cost_tracker=tracker)
        assert client.cost_tracker is tracker


# ── AgentLLMClient.is_available ────────────────────────────────────


class TestIsAvailable:
    @patch("symeraseme.llm.agent_client._detect_agent_backend", return_value="claude")
    def test_available(self, mock_detect: MagicMock) -> None:
        client = AgentLLMClient()
        assert client.is_available() is True
        mock_detect.assert_called_once_with("")

    @patch("symeraseme.llm.agent_client._detect_agent_backend", return_value=None)
    def test_not_available(self, mock_detect: MagicMock) -> None:
        client = AgentLLMClient()
        assert client.is_available() is False

    @patch("symeraseme.llm.agent_client._detect_agent_backend", return_value="claude")
    def test_cached(self, mock_detect: MagicMock) -> None:
        client = AgentLLMClient()
        # First call
        client.is_available()
        # Second call — should not call detect again
        client.is_available()
        assert mock_detect.call_count == 1


# ── AgentLLMClient._call_api ───────────────────────────────────────


class TestCallApi:
    def _make_client(self, **kwargs) -> AgentLLMClient:
        return AgentLLMClient(**kwargs)

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_success(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(
            returncode=0,
            stdout="Hello from Claude",
            stderr="",
        )
        client = self._make_client()
        text, record = client._call_api(
            system_prompt="sys",
            user_prompt="user",
            max_tokens=512,
            temperature=0.0,
            cache_key=None,
        )
        assert text == "Hello from Claude"
        assert record.model == "agent:claude"
        assert record.input_tokens == 0
        assert record.cost == 0.0
        assert len(client.cost_tracker) == 1

    @patch.object(AgentLLMClient, "_resolve_backend", return_value=None)
    def test_no_backend_raises(self, mock_resolve: MagicMock) -> None:
        client = self._make_client()
        with pytest.raises(LLMClientError, match="No host agent CLI detected"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_nonzero_exit_code(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(
            returncode=1,
            stdout="",
            stderr="Error: model not loaded",
        )
        client = self._make_client()
        with pytest.raises(LLMClientError, match="exited with code 1"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_empty_response(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(
            returncode=0,
            stdout="",
            stderr="",
        )
        client = self._make_client()
        with pytest.raises(LLMClientError, match="empty response"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_file_not_found(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.side_effect = FileNotFoundError("No such file")
        client = self._make_client()
        with pytest.raises(LLMClientError, match="CLI not found"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_timeout(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="claude", timeout=120)
        client = self._make_client()
        with pytest.raises(LLMClientError, match="timed out"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_os_error(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.side_effect = OSError("permission denied")
        client = self._make_client()
        with pytest.raises(LLMClientError, match="Failed to invoke host agent"):
            client._call_api(
                system_prompt="s", user_prompt="u",
                max_tokens=100, temperature=0.0, cache_key=None,
            )

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_model_forwarded_for_claude(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(returncode=0, stdout="ok", stderr="")
        client = self._make_client(model="opus")
        client._call_api(
            system_prompt="s", user_prompt="u",
            max_tokens=100, temperature=0.0, cache_key=None,
        )
        cmd = mock_run.call_args[0][0]
        assert "--model" in cmd
        assert "opus" in cmd

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="hermes")
    def test_model_not_forwarded_for_non_claude(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(returncode=0, stdout="ok", stderr="")
        client = self._make_client(model="opus")
        client._call_api(
            system_prompt="s", user_prompt="u",
            max_tokens=100, temperature=0.0, cache_key=None,
        )
        cmd = mock_run.call_args[0][0]
        assert "--model" not in cmd

    @patch("symeraseme.llm.agent_client.subprocess.run")
    @patch.object(AgentLLMClient, "_resolve_backend", return_value="claude")
    def test_auto_model_not_forwarded(self, mock_resolve: MagicMock, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(returncode=0, stdout="ok", stderr="")
        client = self._make_client(model="auto")
        client._call_api(
            system_prompt="s", user_prompt="u",
            max_tokens=100, temperature=0.0, cache_key=None,
        )
        cmd = mock_run.call_args[0][0]
        assert "--model" not in cmd


# ── AgentLLMClient._build_combined_prompt ──────────────────────────


class TestBuildCombinedPrompt:
    def test_merges_prompts(self) -> None:
        client = AgentLLMClient()
        result = client._build_combined_prompt("system instructions", "user message")
        assert "system instructions" in result
        assert "---" in result
        assert "user message" in result
        # Check structure: system + newline + newline + --- + newline + newline + user
        lines = result.split("\n")
        assert lines[0] == "system instructions"
        assert lines[2] == "---"
        assert lines[4] == "user message"
