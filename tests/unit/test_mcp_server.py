from __future__ import annotations

import json
from pathlib import Path
import pytest

from symeraseme.mcp_server import redact_content, MCPJSONRPCHandler


def test_redact_content_basic():
    text = "My email is test@example.com and phone is 555-123-4567."
    redacted = redact_content(text)
    assert "test@example.com" not in redacted
    assert "555-123-4567" not in redacted
    assert "t****@e*.com" in redacted or "t***@e*.*" in redacted or "t*@e*" in redacted or "***-***-4567" in redacted


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
        "id": 1
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 1
    assert "tools" in resp["result"]
    assert resp["result"]["tools"][0]["name"] == "redact_file"


def test_mcp_handler_tools_call_redact_file(tmp_path: Path):
    temp_file = tmp_path / "test.txt"
    temp_file.write_text("Hello, my email is jane.doe@example.com", encoding="utf-8")

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "redact_file",
            "arguments": {
                "path": str(temp_file)
            }
        },
        "id": 2
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 2
    assert "error" not in resp
    content_list = resp["result"]["content"]
    assert len(content_list) == 1
    assert content_list[0]["type"] == "text"
    assert "jane.doe@example.com" not in content_list[0]["text"]


def test_mcp_handler_direct_redact_file(tmp_path: Path):
    temp_file = tmp_path / "test.txt"
    temp_file.write_text("Hello, my SSN is 123-45-6789.", encoding="utf-8")

    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {
            "path": str(temp_file)
        },
        "id": 3
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 3
    assert "error" not in resp
    assert "123-45-6789" not in resp["result"]
    assert "***-**-****" in resp["result"]


def test_mcp_handler_file_not_found():
    handler = DummyHandler()
    req = {
        "jsonrpc": "2.0",
        "method": "redact_file",
        "params": {
            "path": "/nonexistent/path/to/file.txt"
        },
        "id": 4
    }
    resp = handler._handle_single_request(req)
    assert resp["jsonrpc"] == "2.0"
    assert resp["id"] == 4
    assert "error" in resp
    assert "File not found" in resp["error"]["message"]
