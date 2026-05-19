from __future__ import annotations

import json
import os
from importlib import resources
from pathlib import Path

import jsonschema
import yaml

from openeraseme.registry.schema import Broker


def _registry_dir() -> Path:
    env_dir = os.environ.get("OPENERASEME_RESOURCES")
    if env_dir:
        return Path(env_dir)
    pkg_root = resources.files("openeraseme")
    candidate = Path(pkg_root) / "registry"
    if candidate.exists() and (candidate / "brokers").exists():
        return candidate
    for parent in Path(pkg_root).parents:
        if (parent / "registry" / "brokers").exists():
            return parent / "registry"
    msg = "Could not find registry directory"
    raise FileNotFoundError(msg)


def _load_broker_schema() -> dict:
    """Load the broker JSON Schema from the registry schemas directory.

    The schema file lives at registry/schemas/broker.schema.json relative to
    the repo root.  It is the single source of truth — pydantic models are
    derived from it (validated through testing).
    """
    schema_path = _registry_dir() / "schemas" / "broker.schema.json"
    if not schema_path.exists():
        msg = f"Broker schema not found at {schema_path}"
        raise FileNotFoundError(msg)
    with open(schema_path) as f:
        return dict(json.load(f))


_BROKER_SCHEMA: dict | None = None


def broker_schema() -> dict:
    global _BROKER_SCHEMA
    if _BROKER_SCHEMA is None:
        _BROKER_SCHEMA = _load_broker_schema()
    return _BROKER_SCHEMA


def load_broker_yaml(path: str | Path) -> Broker:
    path = Path(path)
    with open(path) as f:
        data = yaml.safe_load(f)
    jsonschema.validate(data, broker_schema())
    return Broker.model_validate(data)


def load_broker(broker_id: str) -> Broker:
    for b in load_all_brokers():
        if b.id == broker_id:
            return b
    msg = f"Broker '{broker_id}' not found in registry"
    raise FileNotFoundError(msg)


def load_all_brokers(
    registry_dir: str | Path | None = None,
    jurisdiction: str | None = None,
    priority: str | None = None,
    category: str | None = None,
) -> list[Broker]:
    if registry_dir is None:
        registry_dir = _registry_dir() / "brokers"

    registry_path = Path(registry_dir)
    brokers: list[Broker] = []

    for yml in sorted(registry_path.rglob("*.yaml")):
        if yml.name.startswith("_"):
            continue  # skip _example.yaml etc.
        try:
            broker = load_broker_yaml(yml)
        except (yaml.YAMLError, jsonschema.ValidationError, Exception):
            continue

        if jurisdiction and jurisdiction not in broker.jurisdictions:
            continue
        if priority and broker.priority.value != priority:
            continue
        if category and broker.category.value != category:
            continue

        brokers.append(broker)

    return brokers
