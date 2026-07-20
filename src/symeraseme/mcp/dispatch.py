"""MCP tool dispatch — resolves handlers and converts CliResult to JSON-RPC."""

from __future__ import annotations

import asyncio
import importlib
import inspect
import json
import logging
import os
from typing import Any

logger = logging.getLogger(__name__)

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
