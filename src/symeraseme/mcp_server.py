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

import asyncio
import importlib
import inspect
import json
import logging
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from symeraseme.cli.console import print_info, print_success

logger = logging.getLogger(__name__)

# Maximum allowed MCP request body size (5 MiB). Requests larger than this are
# rejected with HTTP 413 before the body is read into memory.
MAX_BODY = 5 * 1024 * 1024


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
    except Exception as e:
        return {
            "jsonrpc": "2.0",
            "error": {
                "code": -32603,
                "message": f"Internal error during redaction: {str(e)}",
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
# Tool Registry
# ---------------------------------------------------------------------------

# Tool definitions returned by ``tools/list``.  Each entry follows the MCP
# tool schema: name, description, and inputSchema (JSON Schema object).
TOOL_DEFS: list[dict[str, Any]] = [
    # -- PII Redaction --------------------------------------------------------
    {
        "name": "redact_file",
        "description": (
            "Reads a file, runs PII redaction on it, and returns the redacted content."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to redact",
                },
            },
            "required": ["path"],
        },
    },
    # -- Campaign Planning ----------------------------------------------------
    {
        "name": "plan_create",
        "description": (
            "Create a removal campaign plan selecting brokers by jurisdiction and law."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "jurisdiction": {
                    "type": "string",
                    "description": "Filter by jurisdiction (e.g. GDPR, CCPA)",
                },
                "law": {
                    "type": "string",
                    "description": "Filter by specific law",
                },
                "priority": {
                    "type": "string",
                    "description": "Filter by priority level",
                },
                "max_brokers": {
                    "type": "integer",
                    "description": "Maximum number of brokers to include",
                    "default": 30,
                },
            },
            "required": ["campaign_id"],
        },
    },
    {
        "name": "plan_show",
        "description": "Show the current removal campaign plan.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "status": {
                    "type": "string",
                    "description": "Filter by request status",
                },
            },
        },
    },
    {
        "name": "execute",
        "description": ("Execute a removal campaign by sending opt-out requests in batches."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "account": {
                    "type": "string",
                    "description": "Email account name (himalaya backend)",
                },
                "batch_size": {
                    "type": "integer",
                    "description": "Number of requests per batch",
                    "default": 5,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview actions without sending",
                    "default": False,
                },
                "backend": {
                    "type": "string",
                    "enum": ["smtp", "himalaya"],
                    "description": "Email backend to use",
                },
                "concurrent": {
                    "type": "boolean",
                    "description": "Use concurrent execution",
                    "default": False,
                },
                "workers": {
                    "type": "integer",
                    "description": "Number of concurrent workers",
                    "default": 3,
                },
            },
            "required": ["campaign_id"],
        },
    },
    # -- Inbox Triage ---------------------------------------------------------
    {
        "name": "poll_inbox",
        "description": ("Poll IMAP inbox for broker replies and match them to removal requests."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "host": {
                    "type": "string",
                    "description": "IMAP server hostname",
                },
                "port": {
                    "type": "integer",
                    "description": "IMAP server port",
                },
                "username": {
                    "type": "string",
                    "description": "IMAP username (email address)",
                },
                "since_days": {
                    "type": "integer",
                    "description": "Fetch messages from the last N days",
                },
                "ssl": {
                    "type": "boolean",
                    "description": "Use SSL/TLS connection",
                    "default": True,
                },
                "campaign_id": {
                    "type": "string",
                    "description": "Filter by campaign",
                },
                "folders": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": (
                        "IMAP folders to poll (default: ['INBOX']). "
                        "Deduplicates by Message-ID across folders."
                    ),
                },
            },
            "required": ["host", "port", "username", "since_days", "ssl"],
        },
    },
    {
        "name": "classify_reply",
        "description": (
            "Classify a broker reply using LLM (e.g. confirmation, rejection, info request)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "provider": {
                    "type": "string",
                    "description": "LLM provider override",
                },
                "model": {
                    "type": "string",
                    "description": "LLM model override",
                },
                "save": {
                    "type": "boolean",
                    "description": "Save classification to database",
                    "default": True,
                },
            },
            "required": ["request_id"],
        },
    },
    {
        "name": "generate_rebuttal",
        "description": ("Generate a jurisdiction-aware rebuttal for a broker rejection."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "provider": {
                    "type": "string",
                    "description": "LLM provider override",
                },
                "model": {
                    "type": "string",
                    "description": "LLM model override",
                },
                "save": {
                    "type": "boolean",
                    "description": "Save rebuttal event to database",
                    "default": True,
                },
            },
            "required": ["request_id"],
        },
    },
    # -- Reporting ------------------------------------------------------------
    {
        "name": "generate_dashboard",
        "description": "Generate an HTML dashboard with campaign analytics.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "output": {
                    "type": "string",
                    "description": "Output file path",
                    "default": "report.html",
                },
                "auto_open": {
                    "type": "boolean",
                    "description": "Open dashboard in browser after generation",
                    "default": False,
                },
                "auto_refresh": {
                    "type": "integer",
                    "description": "Auto-refresh interval in seconds (0 = disabled)",
                    "default": 0,
                },
            },
        },
    },
    {
        "name": "generate_report",
        "description": ("Generate a campaign report in HTML, JSON, or CSV format."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "format": {
                    "type": "string",
                    "enum": ["html", "json", "csv"],
                    "description": "Report format",
                    "default": "html",
                },
                "output": {
                    "type": "string",
                    "description": "Output file path",
                },
                "all_campaigns": {
                    "type": "boolean",
                    "description": "Include all campaigns",
                    "default": False,
                },
            },
        },
    },
    # -- Manual Tasks ---------------------------------------------------------
    {
        "name": "manual_tasks_list",
        "description": ("List manual fallback tasks for forms that could not be automated."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by task status",
                },
                "request_id": {
                    "type": "integer",
                    "description": "Filter by request ID",
                },
            },
        },
    },
    {
        "name": "manual_tasks_show",
        "description": "Show details of a specific manual task.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "integer",
                    "description": "Manual task ID",
                },
            },
            "required": ["task_id"],
        },
    },
    {
        "name": "manual_tasks_complete",
        "description": "Mark a manual task as completed.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "integer",
                    "description": "Manual task ID",
                },
                "notes": {
                    "type": "string",
                    "description": "Completion notes",
                    "default": "",
                },
            },
            "required": ["task_id"],
        },
    },
    {
        "name": "manual_tasks_cleanup",
        "description": ("Remove old screenshot and HTML snapshot files from manual tasks."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without deleting",
                    "default": False,
                },
            },
        },
    },
    # -- Scheduler ------------------------------------------------------------
    {
        "name": "generate_scheduler",
        "description": ("Generate cron, launchd, or systemd scheduler configurations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
                "output_dir": {
                    "type": "string",
                    "description": "Output directory for config files",
                    "default": "./schedules",
                },
                "tick_hour": {
                    "type": "integer",
                    "description": "Hour to run tick engine",
                    "default": 10,
                },
                "tick_minute": {
                    "type": "integer",
                    "description": "Minute to run tick engine",
                    "default": 0,
                },
                "poll_hours": {
                    "type": "string",
                    "description": "Comma-separated hours for inbox polling",
                    "default": "8,12,16,20",
                },
                "project_dir": {
                    "type": "string",
                    "description": "Project directory path",
                },
                "symeraseme_bin": {
                    "type": "string",
                    "description": "Path to symeraseme binary",
                },
                "venv_activate": {
                    "type": "string",
                    "description": "Virtualenv activate script path",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without writing files",
                    "default": False,
                },
            },
        },
    },
    {
        "name": "schedule_install",
        "description": "Generate and install scheduler configurations.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
                "tick_hour": {
                    "type": "integer",
                    "description": "Hour to run tick engine",
                    "default": 10,
                },
                "tick_minute": {
                    "type": "integer",
                    "description": "Minute to run tick engine",
                    "default": 0,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without installing",
                    "default": False,
                },
            },
        },
    },
    {
        "name": "schedule_uninstall",
        "description": ("Get instructions for uninstalling scheduler configurations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
            },
        },
    },
    {
        "name": "schedule_status",
        "description": "Check status of installed scheduler services.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
            },
        },
    },
    # -- Registry Validation --------------------------------------------------
    {
        "name": "validate",
        "description": (
            "Validate broker registry YAML files against the JSON Schema and Pydantic model."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "registry_dir": {
                    "type": "string",
                    "description": "Path to registry directory",
                },
            },
        },
    },
    # -- Web Forms ------------------------------------------------------------
    {
        "name": "run_web_form",
        "description": ("Run a broker web-form opt-out via Playwright browser automation."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "broker_id": {
                    "type": "string",
                    "description": "Broker identifier",
                },
                "headed": {
                    "type": "boolean",
                    "description": "Run browser in headed mode (visible)",
                    "default": False,
                },
                "screenshot_dir": {
                    "type": "string",
                    "description": "Directory for screenshots",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without running",
                    "default": False,
                },
            },
            "required": ["broker_id"],
        },
    },
    # -- Auto-Confirm ---------------------------------------------------------
    {
        "name": "auto_confirm",
        "description": "Auto-click confirmation links in broker reply emails.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "headed": {
                    "type": "boolean",
                    "description": "Run browser in headed mode",
                    "default": False,
                },
                "screenshot_dir": {
                    "type": "string",
                    "description": "Directory for screenshots",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without clicking",
                    "default": False,
                },
            },
            "required": ["request_id"],
        },
    },
    # -- Consent Tokens -------------------------------------------------------
    {
        "name": "grant",
        "description": ("Issue, revoke, or list consent tokens for destructive operations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command to grant consent for",
                    "default": "execute",
                },
                "ttl": {
                    "type": "integer",
                    "description": "Token time-to-live in seconds",
                    "default": 86400,
                },
                "revoke": {
                    "type": "string",
                    "description": "Token value to revoke",
                },
                "revoke_all": {
                    "type": "boolean",
                    "description": "Revoke all active tokens",
                    "default": False,
                },
                "list_tokens": {
                    "type": "boolean",
                    "description": "List all active tokens",
                    "default": False,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without issuing or revoking",
                    "default": False,
                },
            },
        },
    },
]

# Fast lookup set for tools/call validation
_TOOL_DEFS_MAP: dict[str, dict[str, Any]] = {t["name"]: t for t in TOOL_DEFS}

# ---------------------------------------------------------------------------
# Tool Dispatch
# ---------------------------------------------------------------------------

# Lazy handler import map: tool_name -> (module_path, function_name).
# Handlers are imported on first call so missing optional extras (web, triage)
# only fail when the tool is actually invoked, not at server startup.
_HANDLER_MAP: dict[str, tuple[str, str]] = {
    "plan_create": ("symeraseme.services.campaign", "handle_plan_create"),
    "plan_show": ("symeraseme.services.campaign", "handle_plan_show"),
    "execute": ("symeraseme.services.campaign", "handle_execute"),
    "poll_inbox": ("symeraseme.services.inbox", "handle_poll_inbox"),
    "classify_reply": ("symeraseme.services.reply", "handle_classify_reply"),
    "generate_rebuttal": ("symeraseme.services.reply", "handle_generate_rebuttal"),
    "generate_dashboard": ("symeraseme.services.reporting", "handle_generate_dashboard"),
    "generate_report": ("symeraseme.services.reporting", "handle_generate_report"),
    "manual_tasks_list": (
        "symeraseme.services.manual_task",
        "handle_manual_tasks_list",
    ),
    "manual_tasks_show": (
        "symeraseme.services.manual_task",
        "handle_manual_tasks_show",
    ),
    "manual_tasks_complete": (
        "symeraseme.services.manual_task",
        "handle_manual_tasks_complete",
    ),
    "manual_tasks_cleanup": (
        "symeraseme.services.manual_task",
        "handle_manual_tasks_cleanup",
    ),
    "generate_scheduler": (
        "symeraseme.services.scheduler",
        "handle_generate_scheduler",
    ),
    "schedule_install": (
        "symeraseme.services.scheduler",
        "handle_schedule_install",
    ),
    "schedule_uninstall": (
        "symeraseme.services.scheduler",
        "handle_schedule_uninstall",
    ),
    "schedule_status": (
        "symeraseme.services.scheduler",
        "handle_schedule_status",
    ),
    "validate": ("symeraseme.services.validate", "handle_validate"),
    "run_web_form": ("symeraseme.services.web_form", "handle_run_web_form"),
    "auto_confirm": ("symeraseme.services.auto_confirm", "handle_auto_confirm"),
    "grant": ("symeraseme.services.consent", "handle_grant"),
}

# Tools that need auto-injected kwargs (consent / yes flags) so that MCP
# invocations skip interactive prompts that would block the server.
_TOOL_AUTO_KWARGS: dict[str, dict[str, Any]] = {
    "execute": {"yes": True},
    "classify_reply": {"yes": True},
    "generate_rebuttal": {"yes": True},
    "schedule_install": {"yes": True},
}

# Handler cache to avoid repeated importlib calls
_handler_cache: dict[str, Any] = {}


def _get_handler(name: str) -> Any:
    """Lazily import and return the handler function for *name*."""
    if name in _handler_cache:
        return _handler_cache[name]

    module_path, func_name = _HANDLER_MAP[name]
    module = importlib.import_module(module_path)
    handler = getattr(module, func_name)
    _handler_cache[name] = handler
    return handler


def _cli_result_to_jsonrpc(result: Any, req_id: Any) -> dict[str, Any]:
    """Convert a CliResult to a JSON-RPC response with MCP content wrapping."""
    from symeraseme.core.result_types import CliResult as CliResultType

    if isinstance(result, CliResultType):
        if result.success:
            return {
                "jsonrpc": "2.0",
                "result": {
                    "content": [{"type": "text", "text": result.to_json()}],
                },
                "id": req_id,
            }
        return {
            "jsonrpc": "2.0",
            "error": {
                "code": -32603,
                "message": result.error or "Handler returned an error",
            },
            "id": req_id,
        }
    # Fallback for unexpected return types
    return {
        "jsonrpc": "2.0",
        "result": {
            "content": [{"type": "text", "text": json.dumps(result, default=str)}],
        },
        "id": req_id,
    }


def _call_tool(name: str, arguments: dict[str, Any], req_id: Any) -> dict[str, Any]:
    """Dispatch a ``tools/call`` request to the registered handler."""
    handler = _get_handler(name)

    # Inject auto-kwargs (consent / yes flags) — setdefault so the caller
    # can still override when explicitly needed.
    auto = _TOOL_AUTO_KWARGS.get(name, {})
    for key, value in auto.items():
        arguments.setdefault(key, value)

    # poll_inbox reads password from the environment to avoid exposing it
    # in the MCP tool schema.
    if name == "poll_inbox":
        arguments.setdefault("password", os.environ.get("IMAP_PASSWORD", ""))

    try:
        if inspect.iscoroutinefunction(handler):
            result = asyncio.run(handler(**arguments))
        else:
            result = handler(**arguments)
        return _cli_result_to_jsonrpc(result, req_id)
    except Exception as e:
        logger.exception("Tool %s raised an exception", name)
        return {
            "jsonrpc": "2.0",
            "error": {
                "code": -32603,
                "message": f"Internal error in {name}: {e}",
            },
            "id": req_id,
        }


# ---------------------------------------------------------------------------
# HTTP JSON-RPC Handler
# ---------------------------------------------------------------------------


class MCPJSONRPCHandler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:
        # Prevent standard http server logging to stdout/stderr unless debug is on
        logger.debug(format, *args)

    def do_POST(self) -> None:
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
    server = ThreadingHTTPServer((host, port), MCPJSONRPCHandler)
    logger.info("Starting MCP Server on http://%s:%d", host, port)
    print_success(f"MCP Server running on http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        logger.info("Stopping MCP Server")
        print_info("Stopping MCP Server...")
    finally:
        server.server_close()
