#!/usr/bin/env python3
"""CI guard: verify MCP tool JSON schemas match their handler function signatures.

This script imports every handler registered in ``_HANDLER_MAP``, compares its
parameter names and required/optional status against the corresponding entry in
``TOOL_DEFS``, and exits non-zero on any discrepancy.

Tools without a handler (e.g. ``redact_file`` which is handled inline in
``mcp_server.py``) are skipped.  Handlers whose signatures use ``*args`` /
``**kwargs`` (after unwrapping decorators) are also skipped — their schema is
the source of truth.

Handlers that cannot be imported (missing optional extras) are reported as
warnings but do **not** fail the check — the CI environment installs all extras.
"""

from __future__ import annotations

import importlib
import inspect
import sys
from typing import Any


def _unwrap_decorators(fn: Any) -> Any:
    """Walk through decorator wrappers to find the original function.

    Handles ``@with_db`` (no ``__wrapped__`` set) and standard
    ``@functools.wraps`` wrappers.
    """
    while True:
        # Standard @functools.wraps chain
        if hasattr(fn, "__wrapped__"):
            fn = fn.__wrapped__
            continue

        # @with_db and similar closure-based wrappers — the first non-"wrapper"
        # callable in the closure is the original function.
        if hasattr(fn, "__closure__") and fn.__closure__:
            found = False
            for cell in fn.__closure__:
                if callable(cell.cell_contents) and hasattr(cell.cell_contents, "__name__"):
                    inner = cell.cell_contents
                    if inner.__name__ != "wrapper":
                        fn = inner
                        found = True
                        break
            if found:
                continue

        break

    return fn


# Parameters that are intentionally injected by the dispatch layer and are
# *not* part of the public MCP tool schema.
_KNOWN_INJECTED: dict[str, set[str]] = {
    # _TOOL_AUTO_KWARGS — consent/yes flags injected so MCP calls skip prompts
    "execute": {"yes", "web_form_runner", "email_sender"},
    "classify_reply": {"yes"},
    "generate_rebuttal": {"yes"},
    "schedule_install": {"yes"},
    # poll_inbox reads password from IMAP_PASSWORD env var in dispatch.py
    "poll_inbox": {"password"},
}


def _params_from_handler(fn: Any, tool_name: str) -> dict[str, dict[str, Any]]:
    """Extract parameter metadata from *fn*'s signature.

    Returns ``{name: {required, has_default, annotation}}`` for every parameter
    that is not auto-injected.
    """
    injected = _KNOWN_INJECTED.get(tool_name, set())
    try:
        sig = inspect.signature(fn)
    except (ValueError, TypeError):
        return {}

    params: dict[str, dict[str, Any]] = {}
    for name, param in sig.parameters.items():
        if name in injected:
            continue
        if param.kind in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD):
            # *args / **kwargs — schema is the source of truth
            return {}
        params[name] = {
            "required": param.default is inspect.Parameter.empty,
            "annotation": param.annotation,
        }
    return params


def _check_tool(
    tool_def: dict[str, Any],
    handler_fn: Any,
    handler_name: str,
) -> list[str]:
    """Compare a single tool definition against its handler.

    Returns a list of error strings (empty = no discrepancies).
    """
    errors: list[str] = []
    tool_name: str = tool_def["name"]

    unwrapped = _unwrap_decorators(handler_fn)
    params = _params_from_handler(unwrapped, tool_name)

    # If the handler uses *args/**kwargs we can't compare — skip
    if not params:
        return []

    schema = tool_def.get("inputSchema", {})
    schema_props: dict[str, Any] = schema.get("properties", {})
    schema_required: set[str] = set(schema.get("required", []))

    # Check every handler parameter against the schema
    for name, meta in sorted(params.items()):
        if name not in schema_props:
            errors.append(
                f"{tool_name}: handler param '{name}' missing from MCP schema"
            )
            continue

        if meta["required"] and name not in schema_required:
            errors.append(
                f"{tool_name}: handler param '{name}' is required (no default) "
                f"but not in schema.required"
            )
        elif not meta["required"] and name in schema_required:
            errors.append(
                f"{tool_name}: handler param '{name}' has a default value "
                f"but is listed in schema.required"
            )

    # Check for schema properties not in the handler
    handler_names = set(params.keys())
    for sp in sorted(schema_props.keys() - handler_names):
        errors.append(
            f"{tool_name}: schema property '{sp}' missing from handler "
            f"signature for {handler_name}"
        )

    return errors


def main() -> int:
    from symeraseme.mcp.dispatch import _HANDLER_MAP
    from symeraseme.mcp.tools import TOOL_DEFS

    tool_schemas = {t["name"]: t for t in TOOL_DEFS}

    all_errors: list[str] = []
    warnings: list[str] = []
    checked = 0
    skipped = 0

    for tool_name in sorted(tool_schemas):
        if tool_name not in _HANDLER_MAP:
            # e.g. redact_file — handled inline in mcp_server.py
            continue

        module_path, func_name = _HANDLER_MAP[tool_name]

        try:
            module = importlib.import_module(module_path)
            handler = getattr(module, func_name)
        except ImportError as exc:
            warnings.append(
                f"{tool_name}: could not import {module_path} ({exc}) — "
                f"skipping (CI installs all extras)"
            )
            skipped += 1
            continue
        except AttributeError:
            all_errors.append(f"{tool_name}: {func_name} not found in {module_path}")
            continue

        errors = _check_tool(tool_schemas[tool_name], handler, func_name)
        if errors:
            all_errors.extend(errors)
        checked += 1

    # Report
    if warnings:
        print(f"[warnings] {len(warnings)} handler(s) skipped (import errors):",
              file=sys.stderr)
        for w in warnings:
            print(f"  - {w}", file=sys.stderr)

    if not all_errors:
        print(
            f"MCP schema check: {checked} tool(s) verified, {skipped} skipped, "
            f"0 discrepancies"
        )
        return 0

    print(
        f"MCP schema check FAILED: {checked} tool(s) checked, "
        f"{len(all_errors)} discrepancy(s):",
        file=sys.stderr,
    )
    for err in all_errors:
        print(f"  - {err}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
