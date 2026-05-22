"""Doctor / environment-check CLI handler."""

from __future__ import annotations

import json
import os
import sys

from openeraseme.core.db import _db_path
from openeraseme.core.identity import _profile_path
from openeraseme.registry.loader import _registry_dir


def _check_python_version() -> tuple[bool, str]:
    version_info = sys.version_info
    ok = version_info >= (3, 11)
    return ok, f"Python {version_info.major}.{version_info.minor}.{version_info.micro}"


def _check_dependencies() -> tuple[bool, str]:
    required = [
        "typer",
        "rich",
        "pydantic",
        "yaml",
        "cryptography",
        "jinja2",
        "jsonschema",
        "structlog",
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


def _check_config_dir() -> tuple[bool, str]:
    try:
        profile_path = _profile_path()
        profile_path.parent.mkdir(parents=True, exist_ok=True)
        test_file = profile_path.parent / ".write_test"
        test_file.write_text("")
        test_file.unlink()
        return True, str(profile_path.parent)
    except Exception as e:
        return False, str(e)


def _check_database() -> tuple[bool, str]:
    try:
        db_path = _db_path()
        db_path.parent.mkdir(parents=True, exist_ok=True)
        return True, str(db_path)
    except Exception as e:
        return False, str(e)


def _check_registry() -> tuple[bool, str]:
    try:
        registry_path = _registry_dir()
        if not registry_path.exists():
            return False, f"Registry not found at {registry_path}"
        broker_count = len(list(registry_path.rglob("*.yaml")))
        return True, f"{broker_count} broker definitions found"
    except Exception as e:
        return False, str(e)


def _check_env_vars() -> tuple[bool, str]:
    optional = [
        "ANTHROPIC_API_KEY",
        "OPENERASEME_ENCRYPT_DB",
        "IMAP_PASSWORD",
        "CAPSOLVER_API_KEY",
    ]
    set_vars = [v for v in optional if os.environ.get(v)]
    if set_vars:
        return True, f"Set: {', '.join(set_vars)}"
    return True, "None set (optional)"


def handle_doctor(output_format: str = "text") -> str:
    """Run environment checks and return formatted results."""
    checks = {
        "Python version": _check_python_version(),
        "Dependencies": _check_dependencies(),
        "Config directory": _check_config_dir(),
        "Database": _check_database(),
        "Registry": _check_registry(),
        "Environment": _check_env_vars(),
    }

    all_ok = all(ok for ok, _ in checks.values())

    if output_format == "json":
        return json.dumps(
            {
                "ok": all_ok,
                "checks": {
                    name: {"ok": ok, "detail": detail}
                    for name, (ok, detail) in checks.items()
                },
            },
            indent=2,
        )

    lines = []
    for name, (ok, detail) in checks.items():
        status = "✓" if ok else "✗"
        lines.append(f"  {status} {name:<20} {detail}")

    header = "Environment check passed" if all_ok else "Environment check failed"
    return f"{header}\n" + "\n".join(lines)
