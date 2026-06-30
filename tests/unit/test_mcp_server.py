from __future__ import annotations

import importlib
import json
from io import BytesIO
from pathlib import Path
from unittest.mock import MagicMock, Mock, patch

import pytest

from symeraseme.core.result_types import CliResult
from symeraseme.mcp_server import (
    MAX_BODY,
    MCPJSONRPCHandler,
    _TOOL_AUTO_KWARGS,
    _cli_result_to_jsonrpc,
    _get_handler,
    _jsonrpc_error,
    _read_workspace_text,
    _run_redaction,
    redact_content,
)


def test_redact_content_basic():
    text = "My email is test@example.com and phone is 555-123-4567."
    redacted = redact_content(text)
    assert "test@example.com" not in redacted
    assert "555-123-4567" not in redacted
    assert (
        "t****@e*.com" in redacted
        or "t***@e*.*" in redacted
        or "t*@e*" in redacted
        or "***-***-4567" in redacted
    )


class DummyHandler:
    """A minimal mock handler to call _handle_single_request."""

    def __init__(self):
        pass

    def _handle_single_request(self, req: dict) -> dict:
        return MCPJSONRPCHandler._handle_single_request(self, req)


def test_mcp_handler_tools_list():
    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 1,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 1
    assert "tools" in resp["result"]
    assert resp["result"]["tools"][0]["name"] == "redact_file"


def test_mcp_handler_tools_call_redact_file(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    temp_file = tmp_path / "test.txt"
    temp_file.write_text("Hello, my email is jane.doe@example.com", encoding="utf-8")

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "redact_file",
            "arguments": {
                "path": "test.txt",
            },
        },
        "id": 2,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 2
    assert "error" not in resp
    content_list = resp["result"]["content"]
    assert len(content_list) == 1
    assert content_list[0]["type"] == "text"
    assert "jane.doe@example.com" not in content_list[0]["text"]


def test_mcp_handler_direct_redact_file(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    temp_file = tmp_path / "test.txt"
    temp_file.write_text("Hello, my SSN is 123-45-6789.", encoding="utf-8")

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {
            "path": "test.txt",
        },
        "id": 3,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 3
    assert "error" not in resp
    assert "123-45-6789" not in resp["result"]
    assert "***-**-****" in resp["result"]


def test_mcp_handler_file_not_found(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {
            "path": "missing.txt",
        },
        "id": 4,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 4
    assert "error" in resp
    assert "File not found" in resp["error"]["message"]


def test_mcp_handler_rejects_absolute_path_outside_workspace(tmp_path: Path, monkeypatch):
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    outside_file = tmp_path / "outside.txt"
    outside_file.write_text("Secret: outside@example.com", encoding="utf-8")
    monkeypatch.chdir(workspace)

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {"path": str(outside_file)},
        "id": 5,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 5
    assert "error" in resp
    assert "outside the MCP workspace" in resp["error"]["message"]


def test_mcp_handler_rejects_directory_traversal(tmp_path: Path, monkeypatch):
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    outside_file = tmp_path / "outside.txt"
    outside_file.write_text("Secret: outside@example.com", encoding="utf-8")
    monkeypatch.chdir(workspace)

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {"path": "../outside.txt"},
        "id": 6,
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 6
    assert "error" in resp
    assert "outside the MCP workspace" in resp["error"]["message"]


def test_mcp_handler_rejects_malformed_content_length():
    handler = MCPJSONRPCHandler.__new__(MCPJSONRPCHandler)
    handler.request = Mock()
    handler.client_address = ("127.0.0.1", 12345)
    handler.server = Mock()
    handler.requestline = "POST / HTTP/1.1"
    handler.request_version = "HTTP/1.1"
    handler.headers = {"Content-Length": "not-a-number"}
    handler.rfile = BytesIO(b"")
    handler.wfile = BytesIO()
    handler.do_POST()
    handler.wfile.seek(0)
    response = handler.wfile.read()
    assert b"-32700" in response
    assert b"Parse error" in response


def test_mcp_handler_rejects_oversized_body():
    handler = MCPJSONRPCHandler.__new__(MCPJSONRPCHandler)
    handler.request = Mock()
    handler.client_address = ("127.0.0.1", 12345)
    handler.server = Mock()
    handler.requestline = "POST / HTTP/1.1"
    handler.request_version = "HTTP/1.1"
    handler.headers = {"Content-Length": str(MAX_BODY + 1)}
    handler.rfile = Mock()
    handler.wfile = BytesIO()
    handler.do_POST()
    handler.rfile.read.assert_not_called()
    handler.wfile.seek(0)
    response = handler.wfile.read()
    assert b"-32600" in response
    assert b"Invalid Request" in response
    assert b"413" in response


# ===========================================================================
# _read_workspace_text — edge cases
# ===========================================================================


class TestReadWorkspaceText:
    """Coverage: _read_workspace_text error paths (lines 44-56)."""

    def test_rejects_non_string_path(self):
        with patch("symeraseme.mcp_server.os") as mock_os:
            with pytest.raises(ValueError, match="Path must be a string"):
                _read_workspace_text(123)

    def test_rejects_null_byte_in_path(self):
        with pytest.raises(ValueError, match="null byte"):
            _read_workspace_text("file\x00.txt")

    def test_reads_file_within_workspace(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        test_file = tmp_path / "hello.txt"
        test_file.write_text("hello world", encoding="utf-8")
        result = _read_workspace_text("hello.txt")
        assert result == "hello world"

    def test_rejects_path_outside_workspace(self, tmp_path: Path, monkeypatch):
        workspace = tmp_path / "workspace"
        workspace.mkdir()
        monkeypatch.chdir(workspace)
        outside = tmp_path / "secret.txt"
        outside.write_text("secret", encoding="utf-8")
        with pytest.raises(ValueError, match="outside the MCP workspace"):
            _read_workspace_text(str(outside))

    def test_file_not_found(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        with pytest.raises(FileNotFoundError):
            _read_workspace_text("nonexistent.txt")


# ===========================================================================
# _jsonrpc_error
# ===========================================================================


class TestJsonrpcError:
    """Coverage: _jsonrpc_error helper (line 129-131)."""

    def test_returns_correct_envelope(self):
        resp = _jsonrpc_error(-32601, "Method not found", 42)
        assert resp["jsonrpc"] == "2.0"
        assert resp["error"]["code"] == -32601
        assert resp["error"]["message"] == "Method not found"
        assert resp["id"] == 42

    def test_none_id(self):
        resp = _jsonrpc_error(-32700, "Parse error", None)
        assert resp["id"] is None


# ===========================================================================
# _cli_result_to_jsonrpc
# ===========================================================================


class TestCliResultToJsonrpc:
    """Coverage: _cli_result_to_jsonrpc (lines 776-804)."""

    def test_success_result_wraps_in_content(self):
        result = CliResult(success=True, data={"count": 5}, message="Done")
        resp = _cli_result_to_jsonrpc(result, 1)
        assert resp["jsonrpc"] == "2.0"
        assert resp["id"] == 1
        assert "error" not in resp
        assert resp["result"]["content"][0]["type"] == "text"
        parsed = json.loads(resp["result"]["content"][0]["text"])
        assert parsed["success"] is True
        assert parsed["count"] == 5

    def test_failure_result_returns_error(self):
        result = CliResult(success=False, error="Something went wrong")
        resp = _cli_result_to_jsonrpc(result, 2)
        assert resp["jsonrpc"] == "2.0"
        assert resp["id"] == 2
        assert resp["error"]["code"] == -32603
        assert resp["error"]["message"] == "Something went wrong"

    def test_failure_result_without_error_message(self):
        result = CliResult(success=False, error=None)
        resp = _cli_result_to_jsonrpc(result, 3)
        assert resp["error"]["message"] == "Handler returned an error"

    def test_unexpected_return_type(self):
        """Handlers that don't return CliResult get fallback serialization."""
        result = {"custom": "data"}
        resp = _cli_result_to_jsonrpc(result, 4)
        assert resp["jsonrpc"] == "2.0"
        assert resp["id"] == 4
        parsed = json.loads(resp["result"]["content"][0]["text"])
        assert parsed["custom"] == "data"

    def test_list_return_type(self):
        result = [1, 2, 3]
        resp = _cli_result_to_jsonrpc(result, 5)
        parsed = json.loads(resp["result"]["content"][0]["text"])
        assert parsed == [1, 2, 3]


# ===========================================================================
# _get_handler — lazy import and caching
# ===========================================================================


class TestGetHandler:
    """Coverage: _get_handler lazy import (lines 764-773)."""

    def setup_method(self):
        """Clear handler cache before each test."""
        from symeraseme.mcp_server import _handler_cache
        _handler_cache.clear()

    def test_imports_and_returns_handler(self):
        handler = _get_handler("plan_create")
        assert callable(handler)

    def test_caches_handler_on_second_call(self):
        from symeraseme.mcp_server import _handler_cache
        handler1 = _get_handler("plan_create")
        handler2 = _get_handler("plan_create")
        assert handler1 is handler2
        assert "plan_create" in _handler_cache

    def test_unknown_tool_raises_key_error(self):
        with pytest.raises(KeyError):
            _get_handler("nonexistent_tool")

    def test_imports_different_handlers(self):
        h1 = _get_handler("plan_create")
        h2 = _get_handler("poll_inbox")
        assert h1 is not h2


# ===========================================================================
# _call_tool — dispatch, auto-kwargs, error handling
# ===========================================================================


class TestCallTool:
    """Coverage: _call_tool dispatch logic (lines 807-837)."""

    def setup_method(self):
        from symeraseme.mcp_server import _handler_cache
        _handler_cache.clear()

    @patch("symeraseme.mcp_server._get_handler")
    def test_calls_handler_with_arguments(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True, data={"ok": True}))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        resp = _call_tool("plan_create", {"campaign_id": "c1"}, 10)
        mock_handler.assert_called_once_with(campaign_id="c1")
        assert resp["result"]["content"][0]["type"] == "text"

    @patch("symeraseme.mcp_server._get_handler")
    def test_injects_auto_kwargs(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        _call_tool("execute", {"campaign_id": "c1"}, 11)
        call_kwargs = mock_handler.call_args[1]
        assert call_kwargs["yes"] is True

    @patch("symeraseme.mcp_server._get_handler")
    def test_auto_kwargs_setdefault_does_not_override(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        _call_tool("execute", {"campaign_id": "c1", "yes": False}, 12)
        call_kwargs = mock_handler.call_args[1]
        assert call_kwargs["yes"] is False

    @patch("symeraseme.mcp_server._get_handler")
    def test_poll_inbox_injects_password_from_env(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        with patch.dict("os.environ", {"IMAP_PASSWORD": "secret123"}):
            _call_tool("poll_inbox", {"host": "imap.test.com", "port": 993, "username": "u", "since_days": 7, "ssl": True}, 13)
        call_kwargs = mock_handler.call_args[1]
        assert call_kwargs["password"] == "secret123"

    @patch("symeraseme.mcp_server._get_handler")
    def test_poll_inbox_password_default_empty(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        with patch.dict("os.environ", {}, clear=True):
            _call_tool("poll_inbox", {"host": "imap.test.com"}, 14)
        call_kwargs = mock_handler.call_args[1]
        assert call_kwargs["password"] == ""

    @patch("symeraseme.mcp_server._get_handler")
    def test_handler_exception_returns_error(self, mock_get):
        mock_handler = Mock(side_effect=RuntimeError("boom"))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        resp = _call_tool("plan_create", {"campaign_id": "c1"}, 15)
        assert resp["error"]["code"] == -32603
        assert "boom" in resp["error"]["message"]

    @patch("symeraseme.mcp_server._get_handler")
    def test_sync_handler_called_directly(self, mock_get):
        mock_handler = Mock(return_value=CliResult(success=True, data={"x": 1}))
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        resp = _call_tool("plan_create", {}, 16)
        mock_handler.assert_called_once()
        assert "error" not in resp

    @patch("symeraseme.mcp_server._get_handler")
    def test_async_handler_runs_via_asyncio(self, mock_get):
        async def async_handler(**kwargs):
            return CliResult(success=True, data={"async": True})

        mock_get.return_value = async_handler
        from symeraseme.mcp_server import _call_tool
        resp = _call_tool("poll_inbox", {"host": "h", "port": 993, "username": "u", "since_days": 1, "ssl": True}, 17)
        assert resp["jsonrpc"] == "2.0"
        parsed = json.loads(resp["result"]["content"][0]["text"])
        assert parsed["async"] is True

    @patch("symeraseme.mcp_server._get_handler")
    def test_non_cliresulter_return_type(self, mock_get):
        mock_handler = Mock(return_value={"custom": "response"})
        mock_get.return_value = mock_handler
        from symeraseme.mcp_server import _call_tool
        resp = _call_tool("plan_create", {}, 18)
        parsed = json.loads(resp["result"]["content"][0]["text"])
        assert parsed["custom"] == "response"


# ===========================================================================
# _run_redaction — success and error paths
# ===========================================================================


class TestRunRedaction:
    """Coverage: _run_redaction (lines 82-121)."""

    def test_success_wraps_content(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        f = tmp_path / "ok.txt"
        f.write_text("Contact: john@test.com", encoding="utf-8")
        resp = _run_redaction("ok.txt", 1, wrap_content=True)
        assert "error" not in resp
        assert resp["result"]["content"][0]["type"] == "text"
        assert "john@test.com" not in resp["result"]["content"][0]["text"]

    def test_success_bare(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        f = tmp_path / "ok.txt"
        f.write_text("Contact: john@test.com", encoding="utf-8")
        resp = _run_redaction("ok.txt", 2, wrap_content=False)
        assert "error" not in resp
        assert "john@test.com" not in resp["result"]

    def test_value_error_returns_invalid_params(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        resp = _run_redaction("nonexistent.txt", 3, wrap_content=True)
        assert resp["error"]["code"] == -32602

    def test_file_not_found_returns_error(self, tmp_path: Path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        resp = _run_redaction("missing.txt", 4, wrap_content=True)
        assert resp["error"]["code"] == -32602
        assert "File not found" in resp["error"]["message"]

    @patch("symeraseme.mcp_server._read_workspace_text")
    def test_generic_exception_returns_internal_error(self, mock_read):
        mock_read.side_effect = OSError("disk failure")
        resp = _run_redaction("any.txt", 5, wrap_content=True)
        assert resp["error"]["code"] == -32603
        assert "Internal error during redaction" in resp["error"]["message"]
        assert "disk failure" in resp["error"]["message"]


# ===========================================================================
# redact_content — profile load failure
# ===========================================================================


class TestRedactContent:
    """Coverage: redact_content profile load paths (lines 62-79)."""

    @patch("symeraseme.core.identity.load_profile")
    @patch("symeraseme.core.identity.profile_exists", return_value=True)
    @patch("symeraseme.adapters.triage.scrubber.scrub_pii")
    def test_profile_load_exception_continues(self, mock_scrub, mock_exists, mock_load):
        mock_load.side_effect = RuntimeError("corrupt profile")
        mock_scrub.return_value = "scrubbed text"
        result = redact_content("hello")
        mock_scrub.assert_called_once_with("hello")

    @patch("symeraseme.core.identity.profile_exists", return_value=False)
    @patch("symeraseme.adapters.triage.scrubber.scrub_pii")
    def test_no_profile_skips_identity_redaction(self, mock_scrub, mock_exists):
        mock_scrub.return_value = "cleaned"
        result = redact_content("hello")
        assert result == "cleaned"


# ===========================================================================
# _handle_single_request — tools/call dispatch to new handlers
# ===========================================================================


class TestHandleToolsCallNewHandlers:
    """Coverage: tools/call dispatch for new handlers added in #476."""

    def setup_method(self):
        from symeraseme.mcp_server import _handler_cache
        _handler_cache.clear()

    @patch("symeraseme.mcp_server._call_tool")
    def test_plan_create_dispatches(self, mock_call):
        mock_call.return_value = {
            "jsonrpc": "2.0",
            "result": {"content": [{"type": "text", "text": "{}"}]},
            "id": 1,
        }
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "plan_create", "arguments": {"campaign_id": "c1"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()
        call_args = mock_call.call_args[0]
        assert call_args[0] == "plan_create"
        assert call_args[1]["campaign_id"] == "c1"

    @patch("symeraseme.mcp_server._call_tool")
    def test_poll_inbox_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "poll_inbox",
                "arguments": {"host": "imap.test.com", "port": 993, "username": "u", "since_days": 7, "ssl": True},
            },
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_generate_dashboard_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "generate_dashboard", "arguments": {"output": "dash.html"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_grant_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "grant", "arguments": {"command": "execute"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_validate_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "validate", "arguments": {}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_run_web_form_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "run_web_form", "arguments": {"broker_id": "spokeo"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_auto_confirm_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "auto_confirm", "arguments": {"request_id": 42}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_manual_tasks_list_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "manual_tasks_list", "arguments": {}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()

    @patch("symeraseme.mcp_server._call_tool")
    def test_generate_scheduler_dispatches(self, mock_call):
        mock_call.return_value = {"jsonrpc": "2.0", "result": {}, "id": 1}
        handler = DummyHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "generate_scheduler", "arguments": {"platform": "cron"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        mock_call.assert_called_once()


# ===========================================================================
# TOOL_DEFS completeness
# ===========================================================================


class TestToolDefsCompleteness:
    """Verify TOOL_DEFS includes all expected tools."""

    def test_all_handler_map_tools_in_tool_defs(self):
        from symeraseme.mcp_server import TOOL_DEFS, _HANDLER_MAP
        tool_names = {t["name"] for t in TOOL_DEFS}
        for name in _HANDLER_MAP:
            assert name in tool_names, f"Handler '{name}' missing from TOOL_DEFS"

    def test_tool_defs_count(self):
        from symeraseme.mcp_server import TOOL_DEFS
        # 21 tools expected from the expansion
        assert len(TOOL_DEFS) >= 20

    def test_all_tools_have_required_fields(self):
        from symeraseme.mcp_server import TOOL_DEFS
        for tool in TOOL_DEFS:
            assert "name" in tool, f"Tool missing 'name'"
            assert "description" in tool, f"Tool {tool.get('name')} missing 'description'"
            assert "inputSchema" in tool, f"Tool {tool.get('name')} missing 'inputSchema'"
            schema = tool["inputSchema"]
            assert schema.get("type") == "object", f"Tool {tool.get('name')} schema type is not 'object'"

    def test_tool_auto_kwargs_all_valid_tools(self):
        from symeraseme.mcp_server import _HANDLER_MAP
        for name in _TOOL_AUTO_KWARGS:
            assert name in _HANDLER_MAP, f"Auto-kwargs tool '{name}' not in handler map"


# ===========================================================================
# Batch dispatch with new tools
# ===========================================================================


class TestBatchDispatchNewTools:
    """Coverage: batch requests dispatching to new handlers."""

    @patch("symeraseme.mcp_server._call_tool")
    def test_batch_mixed_tools(self, mock_call):
        mock_call.side_effect = [
            {"jsonrpc": "2.0", "result": {"content": [{"type": "text", "text": "ok"}]}, "id": 1},
            {"jsonrpc": "2.0", "result": {"content": [{"type": "text", "text": "ok"}]}, "id": 2},
        ]
        handler = DummyHandler()
        reqs = [
            {
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {"name": "plan_create", "arguments": {"campaign_id": "c1"}},
                "id": 1,
            },
            {
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {"name": "generate_dashboard", "arguments": {}},
                "id": 2,
            },
        ]
        # Simulate batch by calling _handle_single_request for each
        resp1 = handler._handle_single_request(reqs[0])
        resp2 = handler._handle_single_request(reqs[1])
        assert mock_call.call_count == 2
        assert resp1["id"] == 1
        assert resp2["id"] == 2


# ===========================================================================
# _TOOL_AUTO_KWARGS completeness
# ===========================================================================


class TestToolAutoKwargs:
    """Verify auto-kwargs configuration for tools that need them."""

    def test_execute_has_yes(self):
        assert "yes" in _TOOL_AUTO_KWARGS["execute"]

    def test_classify_reply_has_yes(self):
        assert "yes" in _TOOL_AUTO_KWARGS["classify_reply"]

    def test_generate_rebuttal_has_yes(self):
        assert "yes" in _TOOL_AUTO_KWARGS["generate_rebuttal"]

    def test_schedule_install_has_yes(self):
        assert "yes" in _TOOL_AUTO_KWARGS["schedule_install"]


# ===========================================================================
# list_tools method alias
# ===========================================================================


class TestListToolsAlias:
    """Coverage: 'list_tools' method alias (line 894)."""

    def test_list_tools_alias_returns_same_as_tools_list(self):
        handler = DummyHandler()
        req_list = {
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 10,
        }
        req_alias = {
            "jsonrpc": "2.0",
            "method": "list_tools",
            "id": 11,
        }
        resp_list = handler._handle_single_request(req_list)
        resp_alias = handler._handle_single_request(req_alias)
        assert resp_list["result"]["tools"] == resp_alias["result"]["tools"]
