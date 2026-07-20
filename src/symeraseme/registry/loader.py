from __future__ import annotations

import contextlib
import hashlib
import hmac
import json
import logging
import os
import time
from importlib import resources
from pathlib import Path
from typing import Any

import jsonschema
import yaml
from pydantic import ValidationError

from symeraseme.core.exceptions import RegistryError
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
    raise RegistryError(msg)


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
_BROKER_VALIDATOR: jsonschema.Draft202012Validator | None = None


def broker_schema() -> dict:
    global _BROKER_SCHEMA
    if _BROKER_SCHEMA is None:
        _BROKER_SCHEMA = _load_broker_schema()
    return _BROKER_SCHEMA


def _broker_validator() -> jsonschema.Draft202012Validator:
    global _BROKER_VALIDATOR
    if _BROKER_VALIDATOR is None:
        _BROKER_VALIDATOR = jsonschema.Draft202012Validator(broker_schema())
    return _BROKER_VALIDATOR


_BROKER_FILE_CACHE: dict[tuple[str, float], Broker] = {}


def load_broker_yaml(path: str | Path) -> Broker:
    path = Path(path)
    mtime = path.stat().st_mtime
    cache_key = (str(path), mtime)
    if cache_key in _BROKER_FILE_CACHE:
        return _BROKER_FILE_CACHE[cache_key]
    with open(path) as f:
        data = yaml.safe_load(f)
    _broker_validator().validate(data)
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
    raise RegistryError(msg)


_BROKER_CACHE: dict[tuple[str, str], list[Broker]] = {}
_SKIPPED_COUNT: dict[tuple[str, str], int] = {}
_BROKER_ID_INDEX: dict[str, Path] = {}


def clear_registry_cache() -> None:
    """Clear all in-memory and persistent broker caches.

    Call this after a registry sync so that subsequent operations
    see the updated data without requiring a process restart.
    """
    global _BROKER_ID_INDEX, _BROKER_SCHEMA, _BROKER_VALIDATOR
    _BROKER_CACHE.clear()
    _BROKER_FILE_CACHE.clear()
    _BROKER_ID_INDEX = {}
    _SKIPPED_COUNT.clear()
    _BROKER_SCHEMA = None
    _BROKER_VALIDATOR = None
    _CACHE_KEY_MEMO.clear()

    _cache_dir = Path.home() / ".cache" / "symeraseme"
    if _cache_dir.exists():
        for ext in ("*.pkl", "*.json"):
            for f in _cache_dir.glob(f"brokers_{ext}"):
                with contextlib.suppress(OSError):
                    f.unlink()


def _cache_dir() -> Path:
    cache = Path.home() / ".cache" / "symeraseme"
    cache.mkdir(parents=True, exist_ok=True, mode=0o700)
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
    canonical = json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
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
        os.chmod(path, 0o600)
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


# How long a computed cache key is trusted before _broker_cache_key re-walks
# the registry directory. Repeated load_all_brokers() calls within one CLI
# invocation (plan -> filter -> lookup) hit this memo instead of re-stat'ing
# ~1,279 files each time. clear_registry_cache() (e.g. after a registry sync)
# forces an immediate recomputation regardless of this TTL.
_CACHE_KEY_TTL_SECONDS = 5.0
_CACHE_KEY_MEMO: dict[str, tuple[float, tuple[str, str]]] = {}


def _broker_cache_key(registry_dir: Path) -> tuple[str, str]:
    dir_str = str(registry_dir)
    now = time.monotonic()
    memoized = _CACHE_KEY_MEMO.get(dir_str)
    if memoized is not None and (now - memoized[0]) < _CACHE_KEY_TTL_SECONDS:
        return memoized[1]

    max_mtime = 0.0
    file_count = 0
    for yml in registry_dir.rglob("*.yaml"):
        try:
            stat = yml.stat()
            if stat.st_mtime > max_mtime:
                max_mtime = stat.st_mtime
            file_count += 1
        except OSError:
            continue
    key_data = f"{registry_dir}:{max_mtime}:{file_count}"
    digest = hashlib.sha256(key_data.encode()).hexdigest()
    cache_key = (str(registry_dir), digest)
    _CACHE_KEY_MEMO[dir_str] = (now, cache_key)
    return cache_key


def _build_broker_id_index(registry_dir: Path) -> dict[str, Path]:
    index: dict[str, Path] = {}
    for yml in registry_dir.rglob("*.yaml"):
        stem = yml.stem
        if not stem or stem.startswith("-") or yml.name.startswith("_"):
            continue
        meta = _meta_only_parse(yml)
        if meta and "id" in meta:
            index[meta["id"]] = yml
        else:
            index[stem] = yml
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


_META_FIELDS: frozenset[str] = frozenset(
    {"jurisdictions", "laws", "priority", "category", "disabled"}
)


def _broker_meta(broker: Broker) -> dict[str, Any]:
    """Build the filterable-fields dict for *broker* (keys: ``_META_FIELDS``)."""
    return {
        "jurisdictions": broker.jurisdictions,
        "laws": [law_item.value for law_item in broker.laws],
        "priority": broker.priority.value,
        "category": broker.category.value,
        "disabled": broker.disabled,
    }


_SAFE_LOADER = yaml.SafeLoader("")


def _meta_only_parse(yml_path: Path) -> dict[str, Any] | None:
    """Extract only the top-level filterable fields without a full YAML parse.

    Uses ``yaml.compose`` to build the node tree and then pulls just the
    five filterable fields (``jurisdictions``, ``laws``, ``priority``,
    ``category``, ``disabled``) off the top-level mapping. This is
    measurably faster than ``yaml.safe_load`` for the cold-cache filter
    path on large registries because it skips the constructor step
    entirely.
    """
    try:
        with open(yml_path) as f:
            node = yaml.compose(f, Loader=yaml.SafeLoader)
    except yaml.YAMLError:
        return None
    if not isinstance(node, yaml.MappingNode):
        return None
    out: dict[str, Any] = {}
    for key_node, value_node in node.value:
        try:
            key = _SAFE_LOADER.construct_object(key_node)
        except yaml.YAMLError:
            continue
        if not isinstance(key, str) or key not in _META_FIELDS:
            continue
        try:
            value = _SAFE_LOADER.construct_object(value_node, deep=True)
        except yaml.YAMLError:
            continue
        out[key] = value
    return out


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


def _filter_and_validate_ymls(
    yml_paths: list[Path],
    *,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> tuple[list[Broker], int]:
    """Load, filter, and validate a list of broker YAML paths.

    Returns (brokers, skipped_count).
    """
    brokers: list[Broker] = []
    skipped = 0
    for yml in yml_paths:
        if yml.name.startswith("_"):
            continue
        meta = _meta_only_parse(yml)
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
    return brokers, skipped


def _load_from_warm_cache(
    cache_key: tuple[str, str],
    *,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> list[Broker]:
    return _filter_brokers(
        _BROKER_CACHE[cache_key],
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )


def _load_from_persistent_cache(
    registry_path: Path,
    cache_key: tuple[str, str],
    has_filters: bool,
    *,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> list[Broker] | None:
    cached = _load_persistent_cache(registry_path, cache_key)
    if cached is None:
        return None
    cached_brokers, cached_index, meta_index = cached
    global _BROKER_ID_INDEX
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
                continue
            brokers.append(broker)
        return brokers
    return None


def _load_cold(
    registry_path: Path,
    cache_key: tuple[str, str],
    has_filters: bool,
    *,
    jurisdiction: str | None = None,
    law: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
) -> list[Broker]:
    yaml_files = sorted(registry_path.rglob("*.yaml"))

    if has_filters:
        filtered_brokers, _ = _filter_and_validate_ymls(
            yaml_files,
            jurisdiction=jurisdiction,
            law=law,
            priority=priority,
            category=category,
            include_disabled=include_disabled,
        )
        return filtered_brokers

    all_brokers: list[Broker] = []
    id_index: dict[str, Path] = {}
    cold_meta_index: dict[str, dict[str, Any]] = {}
    for yml in yaml_files:
        if yml.name.startswith("_"):
            continue
        try:
            broker = load_broker_yaml(yml)
        except (yaml.YAMLError, jsonschema.ValidationError, ValidationError) as exc:
            logger.warning("skipped broker %s: %s", yml, exc)
            continue
        all_brokers.append(broker)
        id_index[broker.id] = yml
        cold_meta_index[broker.id] = _broker_meta(broker)
    global _BROKER_ID_INDEX
    _BROKER_ID_INDEX = id_index
    _save_persistent_cache(registry_path, cache_key, all_brokers, id_index, cold_meta_index)
    return _filter_brokers(
        all_brokers,
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )


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
    if registry_dir is None:
        registry_dir = _registry_dir() / "brokers"

    registry_path = Path(registry_dir)
    cache_key = _broker_cache_key(registry_path)
    has_filters = bool(jurisdiction or law or priority or category)

    if cache_key in _BROKER_CACHE:
        return _load_from_warm_cache(
            cache_key,
            jurisdiction=jurisdiction,
            law=law,
            priority=priority,
            category=category,
            include_disabled=include_disabled,
        )

    result = _load_from_persistent_cache(
        registry_path,
        cache_key,
        has_filters,
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )
    if result is not None:
        return result

    return _load_cold(
        registry_path,
        cache_key,
        has_filters,
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
        meta = _broker_meta(broker)
        if _meta_matches_filters(
            meta,
            jurisdiction=jurisdiction,
            law=law,
            priority=priority,
            category=category,
            include_disabled=include_disabled,
        ):
            filtered.append(broker)
    return filtered
