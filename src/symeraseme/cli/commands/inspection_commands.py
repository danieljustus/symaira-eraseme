"""Inspection & Diagnostics commands."""

from __future__ import annotations

import os
import sys

import typer

from symeraseme import __version__
from symeraseme.cli.console import render_error, render_result
from symeraseme.core.db_connection import _db_path, init_db
from symeraseme.core.events import get_events, list_removal_requests
from symeraseme.core.exceptions import RegistryError
from symeraseme.core.identity import _profile_path
from symeraseme.core.result_types import CliResult
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


def version(ctx: typer.Context) -> None:
    """Show the installed Symaira EraseMe version.

    Examples:
        symeraseme version
    """
    data = {"version": __version__, "name": "Symaira EraseMe"}
    render_result(
        ctx.obj["output"],
        CliResult(data=data, message=f"Symaira EraseMe v{__version__}"),
    )


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
        test_file = db_path.parent / ".write_test"
        test_file.write_text("")
        test_file.unlink()
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


def _check_keyring() -> tuple[bool, str]:
    try:
        import keyring
    except ImportError:
        return False, "keyring package not installed"

    try:
        backend = keyring.get_keyring()
        backend_name = type(backend).__name__
        _unreliable = {"fail", "PlaintextKeyring", "ChainerBackend"}
        if backend_name in _unreliable:
            return False, f"{backend_name} (no secure keyring available)"
        return True, f"{backend_name} (available)"
    except Exception:
        return False, "keyring backend unavailable or errored"


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


_ENV_LABELS: dict[str, str] = {
    "SYMERASEME_LLM_PROVIDER": "LLM provider",
    "SYMERASEME_LLM_MODEL": "LLM model",
    "SYMERASEME_ENCRYPT_DB": "DB encryption",
}

_SENSITIVE_ENV_VARS: dict[str, str] = {
    "IMAP_PASSWORD": "IMAP password",
    "CAPSOLVER_API_KEY": "CAPSOLVER API key",
}

_SENSITIVE_SUFFIXES: tuple[str, ...] = (
    "_PASSWORD",
    "_SECRET",
    "_TOKEN",
    "_KEY",
    "_CREDENTIAL",
)


def _is_sensitive_env_var(name: str) -> bool:
    if name in _SENSITIVE_ENV_VARS:
        return True
    upper = name.upper()
    return any(upper.endswith(suffix) for suffix in _SENSITIVE_SUFFIXES)


def _check_db_encryption() -> tuple[bool, str]:
    from symeraseme.core.db_connection import _db_path
    from symeraseme.core.db_encryption import ENC_HEADER_V1, ENC_MAGIC_V2

    encrypt_enabled = os.environ.get("SYMERASEME_ENCRYPT_DB", "").strip().lower() in (
        "1",
        "true",
        "yes",
    )
    try:
        db_file = _db_path()
    except (OSError, PermissionError):
        if encrypt_enabled:
            return True, "Encryption enabled (DB path inaccessible)"
        return True, "DB path inaccessible"
    if not db_file.exists():
        if encrypt_enabled:
            return True, "Encryption enabled (new DBs will be encrypted)"
        return True, "No database file"

    try:
        raw = db_file.read_bytes()
    except (OSError, PermissionError):
        if encrypt_enabled:
            return True, "Encryption enabled (DB file unreadable)"
        return True, "DB file unreadable"
    is_encrypted = bool(raw) and (raw.startswith(ENC_HEADER_V1) or raw.startswith(ENC_MAGIC_V2))

    if encrypt_enabled and not is_encrypted:
        return False, "Encryption enabled but DB file is plaintext (will encrypt on close)"
    if not encrypt_enabled and is_encrypted:
        return False, "DB file is encrypted but SYMERASEME_ENCRYPT_DB is not set"
    if encrypt_enabled and is_encrypted:
        return True, "Encryption enabled and DB file is encrypted"
    return True, "Encryption not enabled (DB is plaintext)"


def _check_env() -> tuple[bool, str]:
    set_vars = [label for var, label in _ENV_LABELS.items() if os.environ.get(var)]
    sensitive_set = any(_is_sensitive_env_var(var) for var in os.environ)

    pieces: list[str] = []
    if set_vars:
        pieces.extend(set_vars)
    if sensitive_set:
        pieces.append("credentials: configured")

    if pieces:
        return True, "Configured: " + ", ".join(pieces)
    return True, "None set (optional)"


def doctor(ctx: typer.Context) -> None:
    """Run environment checks and report status."""
    checks = {
        "Python version": _check_python_version(),
        "Dependencies": _check_deps(),
        "Config directory": _check_config(),
        "Database": _check_database(),
        "DB encryption": _check_db_encryption(),
        "Registry": _check_registry(),
        "Keyring": _check_keyring(),
        "LLM config": _check_llm(),
        "Environment": _check_env(),
    }

    all_ok = all(ok for ok, _ in checks.values())
    data = {
        "ok": all_ok,
        "checks": {name: {"ok": ok, "detail": detail} for name, (ok, detail) in checks.items()},
    }

    lines = []
    for name, (ok, detail) in checks.items():
        status = "✓" if ok else "✗"
        lines.append(f"  {status} {name:<20} {detail}")
    header = "Environment check passed" if all_ok else "Environment check failed"
    message = f"{header}\n" + "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=data, message=message))


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    """Show the full event history for a removal request.

    Examples:
        symeraseme events show 42
        symeraseme events show 1
    """
    init_db()
    events = get_events(request_id)

    if not events:
        message = f"No events found for request #{request_id}"
    else:
        lines = [f"Events for request #{request_id}:"]
        for e in events:
            lines.append(
                f"  #{e['id']} {e['event_type']} @ {e['occurred_at']} (source: {e['source']})"
            )
        message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data={"events": events}, message=message))


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
    """List removal requests with optional filters and pagination.

    Examples:
        symeraseme requests list
        symeraseme requests list --campaign initial --status SENT
        symeraseme requests list --broker spokeo --page 1 --page-size 50
    """
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

    if not requests:
        message = "No requests found."
    else:
        lines = []
        for r in requests:
            lines.append(
                f"  #{r['id']} [{r.get('current_status', 'N/A')}] "
                f"{r['broker_id']} ({r['campaign_id']})"
            )
        message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=requests, message=message))


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
    brokers = load_all_brokers(
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )

    data = {
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

    if not brokers:
        message = "No brokers match the given filters."
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
        message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=data, message=message))


@brokers_app.command(name="show")
def brokers_show_cmd(
    ctx: typer.Context,
    broker_id: str = typer.Argument(help="Broker id (e.g. acxiom-eu, spokeo)"),
) -> None:
    """Show full details of one broker by id."""
    try:
        broker = load_broker(broker_id)
    except RegistryError:
        render_error(
            f"Broker '{broker_id}' not found in registry. "
            "Run 'symeraseme brokers list' to see available brokers."
        )

    data = {
        "schema_version": 1,
        "broker": broker.model_dump(mode="json", exclude_none=True),
    }

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
    message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=data, message=message))
