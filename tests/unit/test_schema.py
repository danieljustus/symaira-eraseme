"""Tests for broker schema validation and registry loader."""

import json
from pathlib import Path

import jsonschema
import pytest
import yaml

from symeraseme.registry.loader import load_broker_yaml
from symeraseme.registry.schema import Broker


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _load_schema() -> dict:
    schema_path = _repo_root() / "registry" / "schemas" / "broker.schema.json"
    with open(schema_path) as f:
        return dict(json.load(f))


class TestBrokerSchema:
    def test_schema_loads(self):
        schema = _load_schema()
        assert schema["$schema"] == "https://json-schema.org/draft/2020-12/schema"
        assert "properties" in schema

    def test_example_yaml_validates(self):
        schema = _load_schema()
        yml = _repo_root() / "registry" / "brokers" / "eu" / "_example.yaml"
        with open(yml) as f:
            data = yaml.safe_load(f)
        jsonschema.validate(data, schema)

    def test_all_broker_yamls_validate(self):
        schema = _load_schema()
        errors = []
        for yml in sorted((_repo_root() / "registry" / "brokers").rglob("*.yaml")):
            with open(yml) as f:
                data = yaml.safe_load(f)
            try:
                jsonschema.validate(data, schema)
            except jsonschema.ValidationError as e:
                errors.append(f"{yml.name}: {e.message}")
        assert not errors, "Validation errors:\n" + "\n".join(errors)


class TestBrokerModel:
    def test_example_yaml_roundtrip(self):
        yml = _repo_root() / "registry" / "brokers" / "eu" / "_example.yaml"
        broker = load_broker_yaml(yml)
        assert broker.name == "Example Data Broker GmbH"
        assert broker.priority.value == "high"
        assert len(broker.opt_out) == 2

    def test_broker_has_required_fields(self):
        yml = _repo_root() / "registry" / "brokers" / "eu" / "_example.yaml"
        broker = load_broker_yaml(yml)
        assert broker.id
        assert broker.name
        assert broker.website

    def test_email_opt_out_channel(self):
        from symeraseme.registry.schema import EmailOptOut

        yml = _repo_root() / "registry" / "brokers" / "eu" / "acxiom.yaml"
        broker = load_broker_yaml(yml)
        channel = broker.opt_out[0]
        assert isinstance(channel, EmailOptOut)
        assert "@" in channel.endpoint

    def test_web_form_opt_out_channel(self):
        from symeraseme.registry.schema import WebFormOptOut

        yml = _repo_root() / "registry" / "brokers" / "us" / "beenverified.yaml"
        broker = load_broker_yaml(yml)
        channel = broker.opt_out[0]
        assert isinstance(channel, WebFormOptOut)
        assert "steps" in channel.form_spec.model_dump()


class TestRegistryLoader:
    def test_load_all_brokers(self):
        from symeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers()
        assert len(brokers) >= 5
        assert all(isinstance(b, Broker) for b in brokers)

    def test_filter_by_jurisdiction(self):
        from symeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers(jurisdiction="DE")
        assert all("DE" in b.jurisdictions for b in brokers)

    def test_filter_by_priority(self):
        from symeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers(priority="high")
        assert all(b.priority.value == "high" for b in brokers)

    def test_invalid_yaml_rejected(self):
        invalid = _repo_root() / "tests" / "fixtures" / "invalid_broker.yaml"
        if not invalid.exists():
            pytest.skip("fixture not found")

        with open(invalid) as f:
            data = yaml.safe_load(f)
        schema = _load_schema()
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(data, schema)

    def test_persistent_cache_uses_json_not_pickle(self, tmp_path, monkeypatch):
        """Broker cache must be JSON-serialized, never pickle (CVE-230)."""
        import json

        from symeraseme.registry.loader import (
            _broker_cache_key,
            _cache_dir,
            _load_persistent_cache,
            _persistent_cache_path,
            _save_persistent_cache,
            clear_registry_cache,
            load_all_brokers,
        )

        monkeypatch.setattr(
            "symeraseme.registry.loader._cache_dir", lambda: tmp_path
        )

        clear_registry_cache()
        brokers = load_all_brokers(include_disabled=True)
        assert len(brokers) >= 5

        registry_dir = _repo_root() / "registry" / "brokers"
        cache_path = _persistent_cache_path(registry_dir)
        assert cache_path.exists()
        with open(cache_path, encoding="utf-8") as f:
            data = json.load(f)
        assert "cache_key" in data
        assert "brokers" in data
        assert "id_index" in data
        assert all(isinstance(b, dict) for b in data["brokers"])

        loaded = _load_persistent_cache(registry_dir, data["cache_key"])
        assert loaded is not None
        loaded_brokers, loaded_index = loaded
        assert len(loaded_brokers) == len(brokers)
        assert loaded_index

    def test_old_pickle_cache_ignored(self, tmp_path, monkeypatch):
        """Old .pkl caches must be ignored and trigger a rebuild."""
        import pickle

        from symeraseme.registry.loader import (
            _broker_cache_key,
            _cache_dir,
            _load_persistent_cache,
            _persistent_cache_path,
            clear_registry_cache,
        )

        monkeypatch.setattr(
            "symeraseme.registry.loader._cache_dir", lambda: tmp_path
        )
        clear_registry_cache()

        registry_dir = _repo_root() / "registry" / "brokers"
        cache_key = _broker_cache_key(registry_dir)

        pkl_path = tmp_path / "brokers_old.pkl"
        with open(pkl_path, "wb") as f:
            pickle.dump(
                {"cache_key": cache_key, "brokers": [], "id_index": {}}, f
            )

        loaded = _load_persistent_cache(registry_dir, cache_key)
        assert loaded is None


class TestFormDSLStandardization:
    """A5/A6: standardized form_spec DSL — schema rejects the old non-uniform syntax."""

    def _minimal_broker(self, step: dict) -> dict:
        return {
            "id": "test-broker",
            "name": "Test",
            "website": "https://example.com",
            "category": "people-search",
            "jurisdictions": ["US"],
            "laws": ["CCPA"],
            "priority": "low",
            "opt_out": [
                {
                    "type": "web_form",
                    "url": "https://example.com/optout",
                    "form_spec": {"steps": [step]},
                }
            ],
        }

    def test_legacy_selector_from_fill_rejected(self):
        """The old `fill: {selector, from}` shape must fail validation."""
        schema = _load_schema()
        data = self._minimal_broker({"fill": {"selector": "#x", "from": "full_name"}})
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(data, schema)

    def test_dict_fill_accepted(self):
        """The standard `fill: {selector: value}` shape is the only one accepted."""
        schema = _load_schema()
        data = self._minimal_broker({"fill": {"#x": "${full_name}"}})
        jsonschema.validate(data, schema)

    def test_captcha_short_sitekey_rejected(self):
        """Placeholder sitekeys (like '6Lc...') must fail the minLength check."""
        schema = _load_schema()
        data = self._minimal_broker(
            {"solve_captcha": {"type": "recaptcha-v2", "site_key": "6Lc..."}}
        )
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(data, schema)

    def test_captcha_legacy_sitekey_key_rejected(self):
        """The misspelled `sitekey` (no underscore) must fail; only `site_key` allowed."""
        schema = _load_schema()
        data = self._minimal_broker(
            {"solve_captcha": {"type": "recaptcha-v2", "sitekey": "live-key-xxxxxxxx"}}
        )
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(data, schema)

    def test_step_unknown_property_rejected(self):
        """Steps reject unknown top-level keys to catch typos like `fil` for `fill`."""
        schema = _load_schema()
        data = self._minimal_broker({"unknown_action": "value"})
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(data, schema)


class TestDisabledBrokers:
    """A5/A6: known-broken brokers carry `disabled: true` and are filtered by default."""

    def test_disabled_brokers_excluded_by_default(self):
        from symeraseme.registry.loader import load_all_brokers

        active = load_all_brokers()
        assert not any(b.disabled for b in active), (
            "load_all_brokers() must skip disabled brokers by default"
        )

    def test_include_disabled_returns_everything(self):
        from symeraseme.registry.loader import load_all_brokers

        active = load_all_brokers()
        all_brokers = load_all_brokers(include_disabled=True)
        assert len(all_brokers) >= len(active)

    def test_known_disabled_brokers_present(self):
        """beenverified-us, spokeo-eu, whitepages have unverified sitekeys."""
        from symeraseme.registry.loader import load_all_brokers

        all_brokers = load_all_brokers(include_disabled=True)
        by_id = {b.id: b for b in all_brokers}
        for broker_id in ("beenverified-us", "spokeo-eu", "whitepages"):
            assert broker_id in by_id, f"{broker_id} missing from registry"
            assert by_id[broker_id].disabled, f"{broker_id} should be disabled"
            assert by_id[broker_id].notes, f"{broker_id} should explain why it's disabled"
