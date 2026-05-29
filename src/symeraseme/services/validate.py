"""Registry validation CLI handler."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import jsonschema
import yaml

from symeraseme.cli.types import CliResult
from symeraseme.registry.loader import _registry_dir, broker_schema
from symeraseme.registry.schema import Broker


def handle_validate(
    registry_dir: str | None = None,
    output_format: str = "text",
) -> CliResult:
    """Validate every broker YAML against the JSON Schema and the Pydantic model.

    Reports per-file results plus duplicate-id detection.
    """
    brokers_dir = _registry_dir() / "brokers" if registry_dir is None else Path(registry_dir)

    schema = broker_schema()

    valid_files: list[dict[str, str]] = []
    failed_files: list[dict[str, str]] = []
    seen_ids: dict[str, str] = {}
    duplicate_ids: list[dict[str, str]] = []

    for yml in sorted(brokers_dir.rglob("*.yaml")):
        if yml.name.startswith("_"):
            continue
        rel = str(yml.relative_to(brokers_dir.parent.parent))
        try:
            with open(yml) as f:
                data = yaml.safe_load(f)
        except yaml.YAMLError as e:
            failed_files.append({"file": rel, "stage": "yaml", "error": str(e)[:300]})
            continue

        try:
            jsonschema.validate(data, schema)
        except jsonschema.ValidationError as e:
            failed_files.append({"file": rel, "stage": "schema", "error": e.message[:300]})
            continue

        try:
            broker = Broker.model_validate(data)
        except Exception as e:
            failed_files.append({"file": rel, "stage": "pydantic", "error": str(e)[:300]})
            continue

        if broker.id in seen_ids:
            duplicate_ids.append(
                {"id": broker.id, "file": rel, "first_seen_in": seen_ids[broker.id]}
            )
        else:
            seen_ids[broker.id] = rel

        valid_files.append({"file": rel, "id": broker.id, "disabled": str(broker.disabled)})

    summary: dict[str, Any] = {
        "schema_version": 1,
        "registry_dir": str(brokers_dir),
        "totals": {
            "checked": len(valid_files) + len(failed_files),
            "valid": len(valid_files),
            "failed": len(failed_files),
            "duplicate_ids": len(duplicate_ids),
        },
        "failed": failed_files,
        "duplicate_ids": duplicate_ids,
        "valid": valid_files,
    }

    ok = len(failed_files) == 0 and len(duplicate_ids) == 0

    lines = [
        f"Registry: {brokers_dir}",
        f"  Checked: {summary['totals']['checked']}",
        f"  Valid:   {summary['totals']['valid']}",
        f"  Failed:  {summary['totals']['failed']}",
        f"  Duplicate ids: {summary['totals']['duplicate_ids']}",
    ]
    if failed_files:
        lines.append("")
        lines.append("Failures:")
        for fail in failed_files:
            lines.append(f"  {fail['file']}  [{fail['stage']}]  {fail['error']}")
    if duplicate_ids:
        lines.append("")
        lines.append("Duplicate broker ids:")
        for d in duplicate_ids:
            lines.append(f"  {d['id']}: {d['file']} (first seen in {d['first_seen_in']})")
    if ok:
        lines.append("")
        lines.append("OK — registry is valid.")

    summary["ok"] = ok
    summary["message"] = "\n".join(lines)
    return CliResult(
        success=ok,
        data=summary,
        error=None if ok else "Registry validation failed",
    )
