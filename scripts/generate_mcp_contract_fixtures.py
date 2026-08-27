#!/usr/bin/env python3
"""Generate MCP contract golden fixtures from the live TOOL_DEFS.

Writes:
  tests/fixtures/mcp-contract/tools.list.json   — frozen tools/list result
  tests/fixtures/mcp-contract/requests/<tool>.request.json — one valid
      example request per tool, derived from its inputSchema

The generator is deterministic: rerunning it with the same TOOL_DEFS
produces identical files. tests/unit/test_mcp_contract.py fails when the
live surface drifts from the fixtures (run `uv run python
scripts/generate_mcp_contract_fixtures.py` to refresh after an intended
change).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, "src")
from symeraseme.mcp.tools import TOOL_DEFS

FIXTURE_ROOT = Path("tests/fixtures/mcp-contract")
REQUEST_DIR = FIXTURE_ROOT / "requests"


def _example_value(schema: dict, depth: int = 0) -> object:
    """Deterministic example value for a JSON-Schema fragment."""
    if depth > 4:
        return None
    t = schema.get("type")
    if "enum" in schema:
        return schema["enum"][0]
    if "const" in schema:
        return schema["const"]
    if t == "string":
        fmt = schema.get("format")
        if fmt == "email":
            return "user@example.com"
        if fmt == "uri":
            return "https://example.com"
        if fmt == "date":
            return "2026-08-26"
        return "example"
    if t == "integer":
        return 1
    if t == "number":
        return 1.0
    if t == "boolean":
        return True
    if t == "array":
        item = schema.get("items", {})
        return [_example_value(item, depth + 1)]
    if t == "object":
        return {
            k: _example_value(v, depth + 1)
            for k, v in (schema.get("properties") or {}).items()
        }
    return None


def main() -> None:
    REQUEST_DIR.mkdir(parents=True, exist_ok=True)

    # 1. Frozen tools/list result (exact TOOL_DEFS shape).
    (FIXTURE_ROOT / "tools.list.json").write_text(
        json.dumps({"tools": TOOL_DEFS}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    # 2. One valid request per tool.
    for tool in TOOL_DEFS:
        name = tool["name"]
        schema = tool.get("inputSchema", {})
        params = {
            k: _example_value(v)
            for k, v in (schema.get("properties") or {}).items()
        }
        request = {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": name, "arguments": params},
            "id": 1,
        }
        (REQUEST_DIR / f"{name}.request.json").write_text(
            json.dumps(request, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )

    print(f"wrote tools.list.json + {len(TOOL_DEFS)} request fixtures")


if __name__ == "__main__":
    main()
