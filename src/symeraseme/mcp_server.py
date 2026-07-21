"""MCP JSON-RPC server — exposes Symaira EraseMe CLI operations as MCP tools.

Start via ``symeraseme serve``.  The server listens on ``127.0.0.1:8000`` by
default and speaks JSON-RPC 2.0 over plain HTTP.

Tool inventory
--------------
The ``tools/list`` endpoint returns every registered tool.  ``tools/call``
dispatches to the corresponding service handler, converting ``CliResult``
returns into MCP content-wrapped JSON responses.
"""

from __future__ import annotations

import json
import logging
import os
import secrets
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from symeraseme.cli.console import print_info, print_success
from symeraseme.mcp.dispatch import _call_tool
from symeraseme.mcp.tools import _TOOL_DEFS_MAP, TOOL_DEFS

logger = logging.getLogger(__name__)

# Maximum allowed MCP request body size (5 MiB). Requests larger than this are
# rejected with HTTP 413 before the body is read into memory.
MAX_BODY = 5 * 1024 * 1024

# Bearer token required on every request once ``run_mcp_server`` has started
# the server. ``None`` means auth is not configured (e.g. a handler built
# directly in tests without going through ``run_mcp_server``).
_AUTH_TOKEN: str | None = None

# Set when the server is shutting down — gates _is_authorized to reject all
# requests mid-shutdown, closing the race between the finally block clearing
# _AUTH_TOKEN and in-flight requests.
_SHUTDOWN_EVENT = threading.Event()

_ALLOWED_ORIGIN_HOSTS = frozenset({"localhost", "127.0.0.1", "::1"})


def _is_authorized(headers: Any) -> bool:
    """Return True if the request carries a valid ``Authorization: Bearer`` header.

    When ``_AUTH_TOKEN`` is unset, auth is not enforced (test/embedding use).
    Once shutdown begins, all requests are rejected regardless of token state.
    """
    if _SHUTDOWN_EVENT.is_set():
        return False
    if _AUTH_TOKEN is None:
        return True
    auth_header = headers.get("Authorization", "")
    if not auth_header.startswith("Bearer "):
        return False
    token = auth_header[len("Bearer ") :]
    return secrets.compare_digest(token, _AUTH_TOKEN)


def _is_allowed_origin(headers: Any) -> bool:
    """Return True unless the request carries a non-loopback ``Origin`` header.

    Requests without an ``Origin`` header (e.g. same-process clients, curl)
    are allowed; browser-issued cross-origin requests are rejected.
    """
    origin = headers.get("Origin")
    if not origin:
        return True
    hostname = urllib.parse.urlsplit(origin).hostname
    return (hostname or "").lower() in _ALLOWED_ORIGIN_HOSTS


# ---------------------------------------------------------------------------
# File helpers
# ---------------------------------------------------------------------------


def _read_workspace_text(path_str: str, workspace_root: Path | None = None) -> str:
    """Read a user-provided path only when it stays within the MCP workspace.

    Raises ValueError if the normalized path escapes the workspace root.
    """
    if not isinstance(path_str, str):
        raise ValueError("Path must be a string")
    if "\x00" in path_str:
        raise ValueError("Path contains a null byte")

    root = os.path.realpath(os.fspath(workspace_root or Path.cwd()))
    candidate = os.path.realpath(os.path.join(root, os.path.expanduser(path_str)))

    if not candidate.startswith(root):
        raise ValueError("Path is outside the MCP workspace")
    root_prefix = root if root.endswith(os.sep) else root + os.sep
    if candidate != root and not candidate.startswith(root_prefix):
        raise ValueError("Path is outside the MCP workspace")

    with open(candidate, encoding="utf-8") as file:
        return file.read()


def redact_content(text: str) -> str:
    """Run PII redaction on text, using the profile if available and scrub_pii."""
    from symeraseme.adapters.triage.scrubber import scrub_pii
    from symeraseme.core.identity import load_profile, profile_exists
    from symeraseme.core.manual_fallback import _redact_identity_values

    profile = None
    if profile_exists():
        try:
            profile = load_profile()
        except Exception as e:
            logger.debug("Could not load identity profile: %s", e)

    if profile is not None:
        text = _redact_identity_values(text, profile)

    text = scrub_pii(text)
    return text


def _run_redaction(path_str: str, req_id: Any, *, wrap_content: bool) -> dict:
    """Read, redact, and translate errors for a single file path.

    The success envelope is the only difference between the JSON-RPC entry
    points: ``tools/call`` wraps the redacted text in MCP content schema,
    while the bare ``redact_file`` method returns it directly.
    """
    try:
        content = _read_workspace_text(path_str)
        redacted = redact_content(content)
        result = {"content": [{"type": "text", "text": redacted}]} if wrap_content else redacted
        return {
            "jsonrpc": "2.0",
            "result": result,
            "id": req_id,
        }
    except ValueError as e:
        return {
            "jsonrpc": "2.0",
            "error": {"code": -32602, "message": str(e)},
            "id": req_id,
        }
    except FileNotFoundError:
        return {
            "jsonrpc": "2.0",
            "error": {
                "code": -32602,
                "message": f"File not found: {path_str}",
            },
            "id": req_id,
        }
    except Exception:
        logger.exception("Internal error during redaction of %s", path_str)
        return {
            "jsonrpc": "2.0",
            "error": {
                "code": -32603,
                "message": "Internal error during redaction",
            },
            "id": req_id,
        }


# ---------------------------------------------------------------------------
# JSON-RPC helpers
# ---------------------------------------------------------------------------


def _jsonrpc_error(code: int, message: str, req_id: Any) -> dict[str, Any]:
    """Build a JSON-RPC error response."""
    return {"jsonrpc": "2.0", "error": {"code": code, "message": message}, "id": req_id}


# ---------------------------------------------------------------------------
# HTTP JSON-RPC Handler
# ---------------------------------------------------------------------------


class MCPJSONRPCHandler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:
        # Prevent standard http server logging to stdout/stderr unless debug is on
        logger.debug(format, *args)

    def do_POST(self) -> None:
        if not _is_allowed_origin(self.headers):
            self._send_error(-32000, "Forbidden: disallowed Origin", None, status=403)
            return
        if not _is_authorized(self.headers):
            self._send_error(-32000, "Unauthorized", None, status=401)
            return

        content_length_header = self.headers.get("Content-Length", "0")
        try:
            content_length = int(content_length_header)
        except ValueError:
            self._send_error(-32700, "Parse error", None)
            return

        if content_length > MAX_BODY:
            self._send_error(-32600, "Invalid Request", None, status=413)
            return

        post_data = self.rfile.read(content_length)

        try:
            request_data = json.loads(post_data.decode("utf-8"))
        except json.JSONDecodeError:
            self._send_error(-32700, "Parse error", None)
            return

        if isinstance(request_data, list):
            responses = []
            for req in request_data:
                responses.append(self._handle_single_request(req))
            self._send_response_json(responses)
        else:
            response = self._handle_single_request(request_data)
            self._send_response_json(response)

    def _handle_single_request(self, req: dict) -> dict:
        if not isinstance(req, dict) or req.get("jsonrpc") != "2.0":
            return {
                "jsonrpc": "2.0",
                "error": {"code": -32600, "message": "Invalid Request"},
                "id": req.get("id") if isinstance(req, dict) else None,
            }

        method = req.get("method")
        params = req.get("params", {})
        req_id = req.get("id")

        # ------------------------------------------------------------------
        # tools/list — return full tool registry
        # ------------------------------------------------------------------
        if method in ("tools/list", "list_tools"):
            return {
                "jsonrpc": "2.0",
                "result": {"tools": TOOL_DEFS},
                "id": req_id,
            }

        # ------------------------------------------------------------------
        # tools/call — dispatch to handler
        # ------------------------------------------------------------------
        elif method == "tools/call":
            if not isinstance(params, dict):
                return _jsonrpc_error(-32602, "Invalid params", req_id)

            name = params.get("name")
            arguments = params.get("arguments", {})

            if name not in _TOOL_DEFS_MAP:
                return _jsonrpc_error(-32601, "Method not found", req_id)

            # redact_file: use existing _run_redaction for backward compat
            if name == "redact_file":
                path_str = arguments.get("path")
                if not path_str:
                    return _jsonrpc_error(-32602, "Missing required argument: path", req_id)
                return _run_redaction(path_str, req_id, wrap_content=True)

            # All other tools: dispatch through the handler system
            return _call_tool(name, dict(arguments), req_id)

        # ------------------------------------------------------------------
        # Legacy: bare redact_file method (backward compat)
        # ------------------------------------------------------------------
        elif method == "redact_file":
            path_str = None
            if isinstance(params, dict):
                path_str = params.get("path")
            elif isinstance(params, list) and len(params) > 0:
                path_str = params[0]

            if not path_str:
                return _jsonrpc_error(-32602, "Missing path parameter", req_id)

            return _run_redaction(path_str, req_id, wrap_content=False)

        # ------------------------------------------------------------------
        # Unknown method
        # ------------------------------------------------------------------
        else:
            return _jsonrpc_error(-32601, "Method not found", req_id)

    def _send_error(self, code: int, message: str, req_id: Any, *, status: int = 200) -> None:
        self._send_response_json(
            {
                "jsonrpc": "2.0",
                "error": {"code": code, "message": message},
                "id": req_id,
            },
            status=status,
        )

    def _send_response_json(self, data: dict | list, *, status: int = 200) -> None:
        body = json.dumps(data).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


# ---------------------------------------------------------------------------
# Server startup
# ---------------------------------------------------------------------------


def run_mcp_server(host: str = "127.0.0.1", port: int = 8000) -> None:
    global _AUTH_TOKEN
    _AUTH_TOKEN = secrets.token_urlsafe(32)

    from symeraseme.core.config import get_config

    data_dir = get_config().resolved_data_dir
    data_dir.mkdir(parents=True, exist_ok=True)
    token_path = data_dir / "mcp_token"
    fd = os.open(token_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode=0o600)
    with open(fd, "w") as f:
        f.write(_AUTH_TOKEN)

    server = ThreadingHTTPServer((host, port), MCPJSONRPCHandler)
    logger.info("Starting MCP Server on http://%s:%d", host, port)
    print_success(f"MCP Server running on http://{host}:{port}")
    print_info(f"Auth token written to {token_path}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        logger.info("Stopping MCP Server")
        print_info("Stopping MCP Server...")
    finally:
        _SHUTDOWN_EVENT.set()
        _AUTH_TOKEN = None
        server.server_close()
