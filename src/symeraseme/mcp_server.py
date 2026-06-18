from __future__ import annotations

import json
import logging
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

logger = logging.getLogger(__name__)


def redact_content(text: str) -> str:
    """Run PII redaction on text, using the profile if available and scrub_pii."""
    from symeraseme.core.identity import load_profile, profile_exists
    from symeraseme.core.manual_fallback import _redact_identity_values
    from symeraseme.adapters.triage.scrubber import scrub_pii

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


class MCPJSONRPCHandler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: any) -> None:
        # Prevent standard http server logging to stdout/stderr unless debug is on
        logger.debug(format, *args)

    def do_POST(self) -> None:
        content_length = int(self.headers.get("Content-Length", 0))
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

        if method in ("tools/list", "list_tools"):
            return {
                "jsonrpc": "2.0",
                "result": {
                    "tools": [
                        {
                            "name": "redact_file",
                            "description": "Reads a file, runs PII redaction on it, and returns the redacted content.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "The path to the file to redact",
                                    }
                                },
                                "required": ["path"],
                            },
                        }
                    ]
                },
                "id": req_id,
            }

        elif method == "tools/call":
            if not isinstance(params, dict):
                return {
                    "jsonrpc": "2.0",
                    "error": {"code": -32602, "message": "Invalid params"},
                    "id": req_id,
                }
            name = params.get("name")
            arguments = params.get("arguments", {})
            if name != "redact_file":
                return {
                    "jsonrpc": "2.0",
                    "error": {"code": -32601, "message": "Method not found"},
                    "id": req_id,
                }

            path_str = arguments.get("path")
            if not path_str:
                return {
                    "jsonrpc": "2.0",
                    "error": {"code": -32602, "message": "Missing required argument: path"},
                    "id": req_id,
                }

            try:
                path = Path(path_str).expanduser().resolve()
                if not path.exists():
                    return {
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": f"File not found: {path_str}",
                        },
                        "id": req_id,
                    }
                content = path.read_text(encoding="utf-8")
                redacted = redact_content(content)
                return {
                    "jsonrpc": "2.0",
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": redacted,
                            }
                        ]
                    },
                    "id": req_id,
                }
            except Exception as e:
                return {
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32603,
                        "message": f"Internal error during redaction: {str(e)}",
                    },
                    "id": req_id,
                }

        elif method == "redact_file":
            path_str = None
            if isinstance(params, dict):
                path_str = params.get("path")
            elif isinstance(params, list) and len(params) > 0:
                path_str = params[0]

            if not path_str:
                return {
                    "jsonrpc": "2.0",
                    "error": {"code": -32602, "message": "Missing path parameter"},
                    "id": req_id,
                }

            try:
                path = Path(path_str).expanduser().resolve()
                if not path.exists():
                    return {
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": f"File not found: {path_str}",
                        },
                        "id": req_id,
                    }
                content = path.read_text(encoding="utf-8")
                redacted = redact_content(content)
                return {
                    "jsonrpc": "2.0",
                    "result": redacted,
                    "id": req_id,
                }
            except Exception as e:
                return {
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32603,
                        "message": f"Internal error during redaction: {str(e)}",
                    },
                    "id": req_id,
                }

        else:
            return {
                "jsonrpc": "2.0",
                "error": {"code": -32601, "message": "Method not found"},
                "id": req_id,
            }

    def _send_error(self, code: int, message: str, req_id: any) -> None:
        self._send_response_json({
            "jsonrpc": "2.0",
            "error": {"code": code, "message": message},
            "id": req_id,
        })

    def _send_response_json(self, data: dict | list) -> None:
        body = json.dumps(data).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def run_mcp_server(host: str = "127.0.0.1", port: int = 8000) -> None:
    server = HTTPServer((host, port), MCPJSONRPCHandler)
    logger.info("Starting MCP Server on http://%s:%d", host, port)
    print(f"MCP Server running on http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        logger.info("Stopping MCP Server")
        print("\nStopping MCP Server...")
    finally:
        server.server_close()
