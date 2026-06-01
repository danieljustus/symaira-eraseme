"""Inspection & Diagnostics commands."""

from __future__ import annotations

import json
import os
import sys

import typer

from symeraseme import __version__
from symeraseme.cli.console import console, render_error, render_result
from symeraseme.core.db import _db_path, init_db
from symeraseme.core.events import get_events, list_removal_requests
from symeraseme.core.identity import _profile_path
from symeraseme.registry.loader import (
    _SKIPPED_COUNT,
    _broker_cache_key,
    _registry_dir,
    load_all_brokers,
    load_broker,
)
from symeraseme.registry.schema import EmailOptOut, WebFormOptOut

events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
brokers_app = typer.Typer(
    name="brokers",
    help="Discover brokers in the registry (list, show)",
    no_args_is_help=True,
)


def version() -> None:
    console.print(f"Symaira EraseMe v{__version__}", markup=False, soft_wrap=True)


def _check_python_version() -> tuple[bool, str]:
    version_info = sys.version_info
    ok = version_info >= (3, 11)
    return ok, f"Python {version_info.major}.{version_info.minor}.{version_info.micro}"


def _check_deps() -> tuple[bool, str]:
    required = [
        "typer",
        "rich",
        "pydantic",
        "yaml",
        "cryptography",
        "jinja2",
        "jsonschema",
    ]
    missing = []
    for pkg in required:
        try:
            __import__(pkg)
        except ImportError:
            missing.append(pkg)
    if missing:
        return False, f"Missing: {', '.join(missing)}"
    return True, "All required packages installed"


def _check_config() -> tuple[bool, str]:
    try:
        pp = _profile_path()
        pp.parent.mkdir(parents=True, exist_ok=True)
        test_file = pp.parent / ".write_test"
        test_file.write_text("")
        test_file.unlink()
        return True, str(pp.parent)
    except OSError as e:
        return False, str(e)


def _check_database() -> tuple[bool, str]:
    try:
        db_path = _db_path()
        db_path.parent.mkdir(parents=True, exist_ok=True)
        return True, str(db_path)
    except OSError as e:
        return False, str(e)


def _check_registry() -> tuple[bool, str]:
    try:
        rp = _registry_dir()
        if not rp.exists():
            return False, f"Registry not found at {rp}"
        broker_count = len(list(rp.rglob("*.yaml")))
        cache_key = _broker_cache_key(rp)
        skipped = _SKIPPED_COUNT.get(cache_key, 0)
        msg = f"{broker_count} broker definitions found"
        if skipped:
            msg += f" ({skipped} skipped)"
        return True, msg
    except OSError as e:
        return False, str(e)


def _check_llm() -> tuple[bool, str]:
    provider = os.environ.get("SYMERASEME_LLM_PROVIDER", "anthropic")
    key_map = {"anthropic": "ANTHROPIC_API_KEY", "openai": "OPENAI_API_KEY"}
    pieces = [f"provider={provider}"]
    if provider not in key_map and provider != "ollama":
        pieces.append("unknown provider")
        return False, ", ".join(pieces)
    if provider == "ollama":
        pieces.append("(no API key required)")
    else:
        key_var = key_map[provider]
        if os.environ.get(key_var):
            pieces.append(f"{key_var}=✓")
        else:
            pieces.append(f"{key_var}=✗ (not set)")
            return False, ", ".join(pieces)
    model = os.environ.get("SYMERASEME_LLM_MODEL", "")
    if model:
        pieces.append(f"model={model}")
    return True, ", ".join(pieces)


def _check_env() -> tuple[bool, str]:
    optional = [
        "SYMERASEME_LLM_PROVIDER",
        "SYMERASEME_LLM_MODEL",
        "SYMERASEME_ENCRYPT_DB",
        "IMAP_PASSWORD",
        "CAPSOLVER_API_KEY",
    ]
    set_vars = [v for v in optional if os.environ.get(v)]
    if set_vars:
        return True, f"Set: {', '.join(set_vars)}"
    return True, "None set (optional)"


def doctor(ctx: typer.Context) -> None:
    """Run environment checks and report status."""
    checks = {
        "Python version": _check_python_version(),
        "Dependencies": _check_deps(),
        "Config directory": _check_config(),
        "Database": _check_database(),
        "Registry": _check_registry(),
        "LLM config": _check_llm(),
        "Environment": _check_env(),
    }

    all_ok = all(ok for ok, _ in checks.values())

    if ctx.obj["output"] == "json":
        result = json.dumps(
            {
                "ok": all_ok,
                "checks": {
                    name: {"ok": ok, "detail": detail} for name, (ok, detail) in checks.items()
                },
            },
            indent=2,
        )
    else:
        lines = []
        for name, (ok, detail) in checks.items():
            status = "✓" if ok else "✗"
            lines.append(f"  {status} {name:<20} {detail}")
        header = "Environment check passed" if all_ok else "Environment check failed"
        result = f"{header}\n" + "\n".join(lines)

    render_result(ctx.obj["output"], result)


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    import json

    init_db()
    events = get_events(request_id)

    if ctx.obj["output"] == "json":
        result = json.dumps(events, indent=2, default=str)
    elif not events:
        result = f"No events found for request #{request_id}"
    else:
        lines = [f"Events for request #{request_id}:"]
        for e in events:
            lines.append(
                f"  #{e['id']} {e['event_type']} @ {e['occurred_at']} (source: {e['source']})"
            )
        result = "\n".join(lines)

    render_result(ctx.obj["output"], result)


@requests_app.command(name="list")
def requests_list(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        None,
        "--campaign",
        help="Filter by campaign",
    ),
    status: str = typer.Option(None, "--status", help="Filter by status"),
    broker_id: str = typer.Option(
        None,
        "--broker",
        help="Filter by broker ID",
    ),
    page: int = typer.Option(
        None,
        "--page",
        help="Page number (1-based, requires --page-size, default 250)",
    ),
    page_size: int = typer.Option(
        250,
        "--page-size",
        help="Number of results per page (default: 250)",
    ),
) -> None:
    import json

    init_db()
    limit = page_size if page is not None else None
    offset = (page - 1) * page_size if page is not None else None
    requests = list_removal_requests(
        campaign_id=campaign_id,
        status=status,
        broker_id=broker_id,
        limit=limit,
        offset=offset,
    )

    if ctx.obj["output"] == "json":
        result = json.dumps(requests, indent=2, default=str)
    elif not requests:
        result = "No requests found."
    else:
        lines = []
        for r in requests:
            lines.append(
                f"  #{r['id']} [{r.get('current_status', 'N/A')}] "
                f"{r['broker_id']} ({r['campaign_id']})"
            )
        result = "\n".join(lines)

    render_result(ctx.obj["output"], result)


@brokers_app.command(name="list")
def brokers_list_cmd(
    ctx: typer.Context,
    jurisdiction: str = typer.Option(None, help="Filter by jurisdiction (e.g. DE, US, EU)"),
    law: str = typer.Option(
        None,
        help="Filter by law (e.g. GDPR, CCPA, CPRA, LGPD, PIPEDA)",
    ),
    priority: str = typer.Option(None, help="Filter by priority: high, medium, low"),
    category: str = typer.Option(
        None,
        help="Filter by category: people-search, marketing, credit, analytics, "
        "background-check, social-media, other",
    ),
    include_disabled: bool = typer.Option(
        False,
        "--include-disabled",
        help="Include brokers marked disabled (default: skip them).",
    ),
) -> None:
    """List brokers in the registry, optionally filtered."""
    import json

    brokers = load_all_brokers(
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )

    if ctx.obj["output"] == "json":
        payload = {
            "schema_version": 1,
            "filters": {
                "jurisdiction": jurisdiction,
                "law": law,
                "priority": priority,
                "category": category,
                "include_disabled": include_disabled,
            },
            "count": len(brokers),
            "brokers": [
                {
                    "id": b.id,
                    "name": b.name,
                    "website": b.website,
                    "category": b.category.value,
                    "jurisdictions": b.jurisdictions,
                    "laws": [law.value for law in b.laws],
                    "priority": b.priority.value,
                    "data_sensitivity": b.data_sensitivity,
                    "disabled": b.disabled,
                    "opt_out_channels": [ch.type for ch in b.opt_out],
                }
                for b in brokers
            ],
        }
        result = json.dumps(payload, indent=2, default=str)
    elif not brokers:
        result = "No brokers match the given filters."
    else:
        lines = [f"{len(brokers)} broker(s):"]
        for b in brokers:
            channels = "/".join(ch.type for ch in b.opt_out)
            flag = " [DISABLED]" if b.disabled else ""
            juris = ",".join(b.jurisdictions)
            lines.append(
                f"  {b.id:<28} {b.priority.value:<6} {b.category.value:<18} "
                f"{juris:<12} {channels}{flag}"
            )
        result = "\n".join(lines)

    render_result(ctx.obj["output"], result)


@brokers_app.command(name="show")
def brokers_show_cmd(
    ctx: typer.Context,
    broker_id: str = typer.Argument(help="Broker id (e.g. acxiom-eu, spokeo)"),
) -> None:
    """Show full details of one broker by id."""
    import json

    try:
        broker = load_broker(broker_id)
    except FileNotFoundError:
        for b in load_all_brokers(include_disabled=True):
            if b.id == broker_id:
                broker = b
                break
        else:
            render_error(
                f"Broker '{broker_id}' not found in registry. "
                "Run 'symeraseme brokers list' to see available brokers."
            )

    if ctx.obj["output"] == "json":
        result = json.dumps(
            {
                "schema_version": 1,
                "broker": broker.model_dump(mode="json", exclude_none=True),
            },
            indent=2,
            default=str,
        )
    else:
        lines = [
            f"Broker: {broker.name}",
            f"  id:               {broker.id}",
            f"  website:          {broker.website}",
            f"  category:         {broker.category.value}",
            f"  priority:         {broker.priority.value}",
            f"  data_sensitivity: {broker.data_sensitivity}",
            f"  jurisdictions:    {', '.join(broker.jurisdictions)}",
            f"  laws:             {', '.join(law.value for law in broker.laws)}",
            f"  disabled:         {broker.disabled}",
        ]
        for i, channel in enumerate(broker.opt_out, 1):
            lines.append(f"  opt_out[{i}]: {channel.type}")
            if isinstance(channel, EmailOptOut):
                lines.append(f"    endpoint: {channel.endpoint}")
                lines.append(f"    template: {channel.template}")
                lines.append(f"    locale:   {channel.locale}")
                lines.append(f"    expected_response_days: {channel.expected_response_days}")
            elif isinstance(channel, WebFormOptOut):
                lines.append(f"    url:      {channel.url}")
                lines.append(f"    steps:    {len(channel.form_spec.steps)}")
        if broker.verification:
            lines.append(f"  verification.ack_keywords:        {broker.verification.ack_keywords}")
            lines.append(
                f"  verification.rejection_keywords:  {broker.verification.rejection_keywords}"
            )
        if broker.notes:
            lines.append("  notes:")
            for nl in broker.notes.strip().splitlines():
                lines.append(f"    {nl}")
        result = "\n".join(lines)

    render_result(ctx.obj["output"], result)
