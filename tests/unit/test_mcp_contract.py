"""MCP contract drift gate (issue #708).

- The committed tools.list fixture must equal the live TOOL_DEFS surface.
- Every committed request fixture must validate against the live tool's
  inputSchema (so the Go server can be diffed against stable requests).

Regenerate fixtures after an intended surface change:
    uv run python scripts/generate_mcp_contract_fixtures.py
"""

from __future__ import annotations

import json

import jsonschema
import pytest

from symeraseme.mcp.tools import TOOL_DEFS

FIXTURE_ROOT = "tests/fixtures/mcp-contract"
REQUEST_DIR = f"{FIXTURE_ROOT}/requests"


def _load_json(path: str) -> dict:
    with open(path) as f:
        return json.load(f)


def test_tools_list_fixture_matches_live_surface():
    fixture = _load_json(f"{FIXTURE_ROOT}/tools.list.json")
    assert fixture["tools"] == TOOL_DEFS, (
        "tools.list.json drifted from TOOL_DEFS — run "
        "uv run python scripts/generate_mcp_contract_fixtures.py"
    )


@pytest.mark.parametrize("tool", TOOL_DEFS, ids=lambda t: t["name"])
def test_every_tool_has_a_valid_request_fixture(tool):
    name = tool["name"]
    try:
        request = _load_json(f"{REQUEST_DIR}/{name}.request.json")
    except FileNotFoundError:
        pytest.fail(f"missing request fixture for {name} — regenerate fixtures")
    assert request["method"] == "tools/call"
    assert request["params"]["name"] == name
    schema = tool.get("inputSchema", {"type": "object", "properties": {}})
    jsonschema.validate(request["params"]["arguments"], schema)


def test_tool_count_is_pinned():
    # Deliberately pins the 26-tool surface so accidental additions
    # (or removals) are reviewed before the Go port is diffed against it.
    assert len(TOOL_DEFS) == 26
    names = [t["name"] for t in TOOL_DEFS]
    expected = {
        "auto_confirm",
        "classify_reply",
        "execute",
        "generate_dashboard",
        "generate_rebuttal",
        "generate_report",
        "generate_scheduler",
        "get_calendar",
        "get_dashboard_data",
        "get_events",
        "grant",
        "list_brokers",
        "list_requests",
        "manual_tasks_cleanup",
        "manual_tasks_complete",
        "manual_tasks_list",
        "manual_tasks_show",
        "plan_create",
        "plan_show",
        "poll_inbox",
        "redact_file",
        "run_web_form",
        "schedule_install",
        "schedule_status",
        "schedule_uninstall",
        "validate",
    }
    assert set(names) == expected
    assert len(set(names)) == len(names), "tool names must be unique"
