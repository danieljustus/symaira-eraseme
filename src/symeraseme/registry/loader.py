from __future__ import annotations

import hashlib
import hmac
import json
import logging
import os
from importlib import resources
from pathlib import Path
from typing import Any

import jsonschema
import yaml
from pydantic import ValidationError

from symeraseme.registry.schema import Broker

logger = logging.getLogger(__name__)


def _registry_dir() -> Path:
    env_dir = os.environ.get("SYMERASEME_RESOURCES")
    if env_dir:
        return Path(env_dir)
    pkg_root = resources.files("symeraseme")
    candidate = Path(str(pkg_root)) / "registry"
    if candidate.exists() and (candidate / "brokers").exists():
        return candidate
    for parent in Path(str(pkg_root)).parents:
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


_BROKER_FILE_CACHE: dict[tuple[str, float], Broker] = {}


def load_broker_yaml(path: str | Path) -> Broker:
    path = Path(path)
    mtime = path.stat().st_mtime
    cache_key = (str(path), mtime)
    if cache_key in _BROKER_FILE_CACHE:
        return _BROKER_FILE_CACHE[cache_key]
    with open(path) as f:
        data = yaml.safe_load(f)
    jsonschema.validate(data, broker_schema())
    broker = Broker.model_validate(data)
    _BROKER_FILE_CACHE[cache_key] = broker
    return broker


def load_broker(broker_id: str) -> Broker:
    global _BROKER_ID_INDEX
    registry_dir = _registry_dir() / "brokers"
    if broker_id in _BROKER_ID_INDEX:
        return load_broker_yaml(_BROKER_ID_INDEX[broker_id])
    _BROKER_ID_INDEX = _build_broker_id_index(registry_dir)
    if broker_id in _BROKER_ID_INDEX:
        return load_broker_yaml(_BROKER_ID_INDEX[broker_id])
    msg = f"Broker '{broker_id}' not found in registry"
    raise FileNotFoundError(msg)


_BROKER_CACHE: dict[tuple[str, str], list[Broker]] = {}
_SKIPPED_COUNT: dict[tuple[str, str], int] = {}
_BROKER_ID_INDEX: dict[str, Path] = {}


def clear_registry_cache() -> None:
    """Clear all in-memory and persistent broker caches.

    Call this after a registry sync so that subsequent operations
    see the updated data without requiring a process restart.
    """
    global _BROKER_ID_INDEX, _BROKER_SCHEMA
    _BROKER_CACHE.clear()
    _BROKER_FILE_CACHE.clear()
    _BROKER_ID_INDEX = {}
    _SKIPPED_COUNT.clear()
    _BROKER_SCHEMA = None
    import contextlib

    _cache_dir = Path.home() / ".cache" / "symeraseme"
    if _cache_dir.exists():
        for ext in ("*.pkl", "*.json"):
            for f in _cache_dir.glob(f"brokers_{ext}"):
                with contextlib.suppress(OSError):
                    f.unlink()


def _cache_dir() -> Path:
    cache = Path.home() / ".cache" / "symeraseme"
    cache.mkdir(parents=True, exist_ok=True)
    return cache


def _integrity_key() -> bytes | None:
    key_path = _cache_dir() / ".integrity.key"
    if key_path.exists():
        return key_path.read_bytes()
    key = os.urandom(32)
    key_path.write_bytes(key)
    os.chmod(key_path, 0o600)
    return key


def _compute_hmac(payload: dict, key: bytes) -> str:
    canonical = json.dumps(payload, sort_keys=True, ensure_ascii=False)
    return hmac.new(key, canonical.encode("utf-8"), hashlib.sha256).hexdigest()


def _persistent_cache_path(registry_dir: Path) -> Path:
    dir_hash = hashlib.sha256(str(registry_dir).encode()).hexdigest()[:16]
    return _cache_dir() / f"brokers_{dir_hash}.json"


def _save_persistent_cache(
    registry_dir: Path,
    cache_key: tuple[str, str],
    brokers: list[Broker],
    id_index: dict[str, Path],
    meta_index: dict[str, dict[str, Any]] | None = None,
) -> None:
    path = _persistent_cache_path(registry_dir)
    relative_index: dict[str, str] = {}
    for broker_id, broker_path in id_index.items():
        try:
            relative_index[broker_id] = str(broker_path.relative_to(registry_dir))
        except ValueError:
            relative_index[broker_id] = str(broker_path)
    payload: dict[str, Any] = {
        "cache_key": cache_key,
        "brokers": [b.model_dump(mode="json") for b in brokers],
        "id_index": relative_index,
    }
    if meta_index is not None:
        payload["meta_index"] = meta_index
    key = _integrity_key()
    if key is not None:
        payload["integrity"] = _compute_hmac(payload, key)
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(payload, f)
    except OSError as e:
        logger.warning("Failed to save persistent broker cache: %s", e)


def _load_persistent_cache(
    registry_dir: Path,
    cache_key: tuple[str, str],
) -> tuple[list[Broker], dict[str, Path], dict[str, dict[str, Any]] | None] | None:
    path = _persistent_cache_path(registry_dir)
    if not path.exists():
        return None
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        logger.debug("Persistent broker cache unreadable (%s), rebuilding", e)
        return None
    if data.get("cache_key") != cache_key:
        logger.debug("Persistent broker cache key mismatch, rebuilding")
        return None

    stored_hmac = data.pop("integrity", None)
    key = _integrity_key()
    if (
        key is not None
        and stored_hmac is not None
        and not hmac.compare_digest(stored_hmac, _compute_hmac(data, key))
    ):
        logger.warning("Persistent broker cache integrity check failed, rebuilding")
        return None

    raw_brokers = data.get("brokers", [])
    if not raw_brokers:
        return None
    brokers = [Broker.model_validate(b) for b in raw_brokers]
    raw_index = data.get("id_index")
    if isinstance(raw_index, dict) and raw_index:
        id_index = {
            str(broker_id): registry_dir / str(index_path)
            for broker_id, index_path in raw_index.items()
        }
    else:
        id_index = _build_broker_id_index(registry_dir)
    meta_index = data.get("meta_index")
    logger.debug("Loaded %d brokers from persistent cache", len(brokers))
    return brokers, id_index, meta_index


def _broker_cache_key(registry_dir: Path) -> tuple[str, str]:
    dir_stat = registry_dir.stat()
    key_data = f"{registry_dir}:{dir_stat.st_mtime}:{dir_stat.st_size}"
    digest = hashlib.sha256(key_data.encode()).hexdigest()
    return (str(registry_dir), digest)


def _build_broker_id_index(registry_dir: Path) -> dict[str, Path]:
    index: dict[str, Path] = {}
    for yml in registry_dir.rglob("*.yaml"):
        if yml.name.startswith("_"):
            continue
        meta = _quick_parse_meta(yml)
        if meta is not None and "id" in meta:
            index[meta["id"]] = yml
    return index


def _quick_parse_meta(yml_path: Path) -> dict | None:
    """Lightweight YAML parse: extract only filterable fields, no validation."""
    try:
        with open(yml_path) as f:
            data = yaml.safe_load(f)
    except yaml.YAMLError:
        return None
    if not isinstance(data, dict):
        return None
    return data


def _meta_matches_filters(
    meta: dict,
    jurisdiction: str | None,
    law: str | None,
    priority: str | None,
    category: str | None,
    include_disabled: bool,
) -> bool:
    if not include_disabled and meta.get("disabled", False):
        return False
    if jurisdiction and jurisdiction not in meta.get("jurisdictions", []):
        return False
    if law and law not in meta.get("laws", []):
        return False
    if priority and meta.get("priority") != priority:
        return False
    if category and meta.get("category") != category:  # noqa: SIM103
        return False
    return True


def load_all_brokers(
    registry_dir: str | Path | None = None,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> list[Broker]:
    """Load and filter brokers from the registry.

    Disabled brokers (``disabled: true`` in YAML) are excluded by default
    so the default plan never targets known-broken broker entries.
    Pass ``include_disabled=True`` to get everything (used by ``brokers list``).

    When filters are active and the global cache is cold, only YAML files
    matching the filters are loaded — avoiding full parse of all ~1,279
    files for targeted queries like ``--jurisdiction GDPR --max 5``.
    """
    global _BROKER_ID_INDEX
    if registry_dir is None:
        registry_dir = _registry_dir() / "brokers"

    registry_path = Path(registry_dir)
    cache_key = _broker_cache_key(registry_path)
    has_filters = bool(jurisdiction or law or priority or category)

    # Warm in-memory cache: filter from cached brokers.
    if cache_key in _BROKER_CACHE:
        return _filter_brokers(
            _BROKER_CACHE[cache_key],
            jurisdiction=jurisdiction,
            law=law,
            priority=priority,
            category=category,
            include_disabled=include_disabled,
        )

    cached = _load_persistent_cache(registry_path, cache_key)
    if cached is not None:
        cached_brokers, cached_index, meta_index = cached
        _BROKER_ID_INDEX = cached_index
        if not has_filters:
            _BROKER_CACHE[cache_key] = cached_brokers
            return _filter_brokers(
                cached_brokers,
                jurisdiction=jurisdiction,
                law=law,
                priority=priority,
                category=category,
                include_disabled=include_disabled,
            )
        if meta_index is not None:
            brokers: list[Broker] = []
            skipped = 0
            for broker_id, meta_entry in meta_index.items():
                if not _meta_matches_filters(
                    meta_entry,
                    jurisdiction=jurisdiction,
                    law=law,
                    priority=priority,
                    category=category,
                    include_disabled=include_disabled,
                ):
                    continue
                yml = cached_index.get(broker_id)
                if yml is None:
                    continue
                try:
                    broker = load_broker_yaml(yml)
                except (yaml.YAMLError, jsonschema.ValidationError, ValidationError) as exc:
                    logger.warning("skipped broker %s: %s", yml, exc)
                    skipped += 1
                    continue
                brokers.append(broker)
            return brokers

    yaml_files = sorted(registry_path.rglob("*.yaml"))

    if has_filters:
        # Cold cache + filters: lazy-load only matching files.
        brokers = []
        skipped = 0
        for yml in yaml_files:
            if yml.name.startswith("_"):
                continue
            meta = _quick_parse_meta(yml)
            if meta is None:
                logger.warning("skipped broker %s: unparseable YAML", yml)
                skipped += 1
                continue
            if not _meta_matches_filters(
                meta,
                jurisdiction=jurisdiction,
                law=law,
                priority=priority,
                category=category,
                include_disabled=include_disabled,
            ):
                continue
            try:
                broker = load_broker_yaml(yml)
            except (yaml.YAMLError, jsonschema.ValidationError, ValidationError) as exc:
                logger.warning("skipped broker %s: %s", yml, exc)
                skipped += 1
                continue
            brokers.append(broker)
        return brokers

    brokers = []
    skipped = 0
    id_index: dict[str, Path] = {}
    cold_meta_index: dict[str, dict[str, Any]] = {}
    for yml in yaml_files:
        if yml.name.startswith("_"):
            continue
        try:
            broker = load_broker_yaml(yml)
        except (yaml.YAMLError, jsonschema.ValidationError, ValidationError) as exc:
            logger.warning("skipped broker %s: %s", yml, exc)
            skipped += 1
            continue
        brokers.append(broker)
        id_index[broker.id] = yml
        cold_meta_index[broker.id] = {
            "jurisdictions": broker.jurisdictions,
            "laws": [law_item.value for law_item in broker.laws],
            "priority": broker.priority.value,
            "category": broker.category.value,
            "disabled": broker.disabled,
        }
    _BROKER_ID_INDEX = id_index
    _save_persistent_cache(registry_path, cache_key, brokers, id_index, cold_meta_index)
    return _filter_brokers(
        brokers,
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )


def _filter_brokers(
    brokers: list[Broker],
    *,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> list[Broker]:
    filtered: list[Broker] = []
    for broker in brokers:
        if not include_disabled and broker.disabled:
            continue
        if jurisdiction and jurisdiction not in broker.jurisdictions:
            continue
        if law and law not in [law_item.value for law_item in broker.laws]:
            continue
        if priority and broker.priority.value != priority:
            continue
        if category and broker.category.value != category:
            continue
        filtered.append(broker)
    return filtered
