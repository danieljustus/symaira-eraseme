from __future__ import annotations

from io import BytesIO
from pathlib import Path
from unittest.mock import Mock

from symeraseme.mcp_server import MAX_BODY, MCPJSONRPCHandler, redact_content


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
