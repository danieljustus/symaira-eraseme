#!/usr/bin/env python3
"""Dump the current MCP TOOL_DEFS surface as JSON (contract extraction helper)."""
import json
import sys

sys.path.insert(0, "src")
from symeraseme.mcp.tools import TOOL_DEFS

print(json.dumps(TOOL_DEFS, indent=1, ensure_ascii=False))
