#!/usr/bin/env python3
"""Print a compact table of the 26 MCP tools for docs/mcp-contract.md."""
import sys

sys.path.insert(0, "src")
from symeraseme.mcp.tools import TOOL_DEFS

for t in TOOL_DEFS:
    schema = t.get("inputSchema", {})
    props = schema.get("properties", {})
    req = schema.get("required", [])
    prop_lines = []
    for k, v in props.items():
        ptype = v.get("type", "?")
        en = v.get("enum")
        desc = (v.get("description") or "").strip()
        mark = "*" if k in req else " "
        tag = f"enum={en}" if en else ptype
        prop_lines.append(f"    - {mark} `{k}` ({tag}){': ' + desc if desc else ''}")
    print(f"### `{t['name']}`")
    print()
    print((t.get("description") or "").strip())
    print()
    print(f"Required: `{[k for k in props if k in req] or '—'}`")
    print()
    print("Parameters:")
    print("\n".join(prop_lines) if prop_lines else "    - (none)")
    print()
