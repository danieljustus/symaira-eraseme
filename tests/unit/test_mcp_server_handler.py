"""Tests for MCP JSON-RPC HTTP handler (do_POST and dispatch error paths).

Covers lines 121-136 (do_POST single/batch dispatch), 179-187 (tools/call error
paths), and 207-208 (redact_file bare with list params).
"""

from __future__ import annotations

import json
from io import BytesIO
from unittest.mock import Mock, patch

import symeraseme.mcp_server as mcp_server_module
from symeraseme.mcp_server import MCPJSONRPCHandler

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_handler(
    rfile_body: bytes,
    content_length: str | None = None,
    extra_headers: dict[str, str] | None = None,
) -> MCPJSONRPCHandler:
    """Build a minimal MCPJSONRPCHandler instance for do_POST testing.

    Sets all fields that ``BaseHTTPRequestHandler`` normally initialises so
    that ``send_response`` / ``end_headers`` / ``send_header`` can write to
    ``wfile`` without raising ``AttributeError``.
    """
    handler = MCPJSONRPCHandler.__new__(MCPJSONRPCHandler)
    handler.request = Mock()
    handler.client_address = ("127.0.0.1", 12345)
    handler.server = Mock()
    handler.requestline = "POST / HTTP/1.1"
    handler.request_version = "HTTP/1.1"
    headers = {"Content-Length": content_length or str(len(rfile_body))}
    headers.update(extra_headers or {})
    handler.headers = headers
    handler.rfile = BytesIO(rfile_body)
    handler.wfile = BytesIO()
    return handler


def _do_post(handler: MCPJSONRPCHandler) -> dict | list:
    """Run do_POST and return the parsed JSON response body."""
    handler.do_POST()
    handler.wfile.seek(0)
    raw = handler.wfile.read()
    # Strip HTTP headers (everything before \r\n\r\n)
    _, _, body = raw.partition(b"\r\n\r\n")
    return json.loads(body)


class _MethodHandler:
    """Proxy that exposes only ``_handle_single_request`` without any of the
    HTTP-server boilerplate that a real ``MCPJSONRPCHandler`` needs."""

    def _handle_single_request(self, req: dict) -> dict:
        return MCPJSONRPCHandler._handle_single_request(self, req)


# ===========================================================================
# do_POST — single request
# ===========================================================================


class TestDoPostSingle:
    """Coverage: ``do_POST`` lines 121-136 (single-request path)."""

    @patch("symeraseme.mcp_server._run_redaction")
    def test_valid_redact_file_request(self, mock_run):
        mock_run.return_value = {
            "jsonrpc": "2.0",
            "result": "redacted",
            "id": 1,
        }
        req = {
            "jsonrpc": "2.0",
            "method": "redact_file",
            "params": {"path": "test.txt"},
            "id": 1,
        }
        handler = _make_handler(json.dumps(req).encode())
        resp = _do_post(handler)

        assert resp["jsonrpc"] == "2.0"
        assert resp["result"] == "redacted"
        assert resp["id"] == 1
        mock_run.assert_called_once_with("test.txt", 1, wrap_content=False)

    @patch("symeraseme.mcp_server._run_redaction")
    def test_valid_tools_call_request(self, mock_run):
        mock_run.return_value = {
            "jsonrpc": "2.0",
            "result": {"content": [{"type": "text", "text": "redacted"}]},
            "id": 1,
        }
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "redact_file",
                "arguments": {"path": "test.txt"},
            },
            "id": 1,
        }
        handler = _make_handler(json.dumps(req).encode())
        resp = _do_post(handler)

        assert resp["jsonrpc"] == "2.0"
        assert resp["result"]["content"][0]["text"] == "redacted"
        mock_run.assert_called_once_with("test.txt", 1, wrap_content=True)

    def test_malformed_json_body(self):
        handler = _make_handler(b"not-valid-json!!")
        resp = _do_post(handler)

        assert resp["jsonrpc"] == "2.0"
        assert resp["error"]["code"] == -32700
        assert resp["error"]["message"] == "Parse error"
        assert resp["id"] is None

    def test_invalid_jsonrpc_version(self):
        req = {
            "jsonrpc": "1.0",
            "method": "redact_file",
            "params": {"path": "test.txt"},
            "id": 1,
        }
        handler = _make_handler(json.dumps(req).encode())
        resp = _do_post(handler)

        assert resp["jsonrpc"] == "2.0"
        assert resp["error"]["code"] == -32600
        assert resp["error"]["message"] == "Invalid Request"
        assert resp["id"] == 1


# ===========================================================================
# do_POST — batch requests
# ===========================================================================


class TestDoPostBatch:
    """Coverage: ``do_POST`` lines 129-133 (list / batch path)."""

    @patch("symeraseme.mcp_server._run_redaction")
    def test_two_valid_requests(self, mock_run):
        mock_run.side_effect = [
            {"jsonrpc": "2.0", "result": "a", "id": 1},
            {"jsonrpc": "2.0", "result": "b", "id": 2},
        ]
        reqs = [
            {
                "jsonrpc": "2.0",
                "method": "redact_file",
                "params": {"path": "a.txt"},
                "id": 1,
            },
            {
                "jsonrpc": "2.0",
                "method": "redact_file",
                "params": {"path": "b.txt"},
                "id": 2,
            },
        ]
        handler = _make_handler(json.dumps(reqs).encode())
        resp = _do_post(handler)

        assert isinstance(resp, list)
        assert len(resp) == 2
        assert resp[0]["result"] == "a"
        assert resp[1]["result"] == "b"

    def test_batch_with_invalid_jsonrpc_entry(self):
        """A batch where one entry has a bad jsonrpc version."""
        reqs = [
            {
                "jsonrpc": "2.0",
                "method": "redact_file",
                "params": {"path": "ok.txt"},
                "id": 1,
            },
            {
                "jsonrpc": "1.0",
                "method": "redact_file",
                "params": {"path": "bad.txt"},
                "id": 2,
            },
        ]
        handler = _make_handler(json.dumps(reqs).encode())
        resp = _do_post(handler)

        assert isinstance(resp, list)
        assert len(resp) == 2
        # The second entry (bad jsonrpc version) gets Invalid Request
        assert resp[1]["error"]["code"] == -32600
        assert resp[1]["error"]["message"] == "Invalid Request"
        assert resp[1]["id"] == 2

    def test_batch_with_malformed_entry(self):
        """A batch where one entry is a string, not a dict."""
        reqs = [
            {
                "jsonrpc": "2.0",
                "method": "redact_file",
                "params": {"path": "ok.txt"},
                "id": 1,
            },
            "not-a-dict",
        ]
        handler = _make_handler(json.dumps(reqs).encode())
        resp = _do_post(handler)

        assert isinstance(resp, list)
        assert len(resp) == 2
        # The non-dict entry fails with Invalid Request
        assert resp[1]["error"]["code"] == -32600
        assert resp[1]["error"]["message"] == "Invalid Request"
        # id is None because the entry is not a dict
        assert resp[1]["id"] is None


# ===========================================================================
# _handle_single_request — tools/call error paths
# ===========================================================================


class TestHandleToolsCall:
    """Coverage: lines 178-191 (tools/call error dispatch)."""

    def test_params_is_not_dict(self):
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": "not-a-dict",
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32602
        assert resp["error"]["message"] == "Invalid params"

    def test_unknown_tool_name(self):
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "do_something_else",
                "arguments": {},
            },
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32601
        assert resp["error"]["message"] == "Method not found"

    def test_missing_name_key(self):
        """When ``name`` is absent from params, ``None != 'redact_file'``
        is ``True``, so the handler returns "Method not found"."""
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"arguments": {"path": "test.txt"}},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32601
        assert resp["error"]["message"] == "Method not found"

    def test_missing_path_in_arguments(self):
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "redact_file",
                "arguments": {},
            },
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32602
        assert "Missing required argument" in resp["error"]["message"]


# ===========================================================================
# _handle_single_request — bare redact_file error paths
# ===========================================================================


class TestHandleRedactFile:
    """Coverage: lines 203-217 (``redact_file`` bare method dispatch)."""

    @patch("symeraseme.mcp_server._run_redaction")
    def test_with_list_params(self, mock_run):
        """Coverage: lines 207-208 — ``params`` is a list, so
        ``path_str = params[0]``."""
        mock_run.return_value = {
            "jsonrpc": "2.0",
            "result": "redacted",
            "id": 1,
        }
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "redact_file",
            "params": ["test.txt"],
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["result"] == "redacted"
        mock_run.assert_called_once_with("test.txt", 1, wrap_content=False)

    def test_with_empty_list_params(self):
        """Empty list → ``path_str`` stays ``None`` → "Missing path"."""
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "redact_file",
            "params": [],
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32602
        assert resp["error"]["message"] == "Missing path parameter"

    def test_missing_path_in_params(self):
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "redact_file",
            "params": {},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32602
        assert resp["error"]["message"] == "Missing path parameter"


# ===========================================================================
# _handle_single_request — unknown method
# ===========================================================================


class TestHandleUnknownMethod:
    """Coverage: lines 219-224 (fallback ``else`` branch)."""

    def test_unknown_method_name(self):
        handler = _MethodHandler()
        req = {
            "jsonrpc": "2.0",
            "method": "nonexistent",
            "params": {},
            "id": 1,
        }
        resp = handler._handle_single_request(req)
        assert resp["error"]["code"] == -32601
        assert resp["error"]["message"] == "Method not found"


# ===========================================================================
# do_POST — bearer token auth and Origin validation
# ===========================================================================


class TestDoPostAuthAndOrigin:
    """Coverage: the auth-token and Origin gates added ahead of dispatch."""

    def setup_method(self):
        # Simulate a server started via run_mcp_server(), which sets a token.
        mcp_server_module._AUTH_TOKEN = "s3cr3t-token"

    def teardown_method(self):
        mcp_server_module._AUTH_TOKEN = None

    def _valid_body(self) -> bytes:
        req = {
            "jsonrpc": "2.0",
            "method": "redact_file",
            "params": {"path": "test.txt"},
            "id": 1,
        }
        return json.dumps(req).encode()

    def test_missing_auth_header_rejected(self):
        handler = _make_handler(self._valid_body())
        resp = _do_post(handler)

        assert resp["error"]["code"] == -32000
        assert resp["error"]["message"] == "Unauthorized"

    def test_wrong_bearer_token_rejected(self):
        handler = _make_handler(
            self._valid_body(),
            extra_headers={"Authorization": "Bearer wrong-token"},
        )
        resp = _do_post(handler)

        assert resp["error"]["code"] == -32000
        assert resp["error"]["message"] == "Unauthorized"

    @patch("symeraseme.mcp_server._run_redaction")
    def test_correct_bearer_token_accepted(self, mock_run):
        mock_run.return_value = {"jsonrpc": "2.0", "result": "redacted", "id": 1}
        handler = _make_handler(
            self._valid_body(),
            extra_headers={"Authorization": "Bearer s3cr3t-token"},
        )
        resp = _do_post(handler)

        assert resp["result"] == "redacted"

    @patch("symeraseme.mcp_server._run_redaction")
    def test_no_auth_token_configured_allows_request(self, mock_run):
        """When the server was never started via run_mcp_server(), auth is not enforced."""
        mcp_server_module._AUTH_TOKEN = None
        mock_run.return_value = {"jsonrpc": "2.0", "result": "redacted", "id": 1}
        handler = _make_handler(self._valid_body())
        resp = _do_post(handler)

        assert resp["result"] == "redacted"

    def test_disallowed_origin_rejected(self):
        handler = _make_handler(
            self._valid_body(),
            extra_headers={
                "Authorization": "Bearer s3cr3t-token",
                "Origin": "https://evil.example.com",
            },
        )
        resp = _do_post(handler)

        assert resp["error"]["code"] == -32000
        assert resp["error"]["message"] == "Forbidden: disallowed Origin"

    @patch("symeraseme.mcp_server._run_redaction")
    def test_loopback_origin_allowed(self, mock_run):
        mock_run.return_value = {"jsonrpc": "2.0", "result": "redacted", "id": 1}
        handler = _make_handler(
            self._valid_body(),
            extra_headers={
                "Authorization": "Bearer s3cr3t-token",
                "Origin": "http://127.0.0.1:8000",
            },
        )
        resp = _do_post(handler)

        assert resp["result"] == "redacted"
