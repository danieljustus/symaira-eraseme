"""Tests for broker schema validation and registry loader."""

import json
from pathlib import Path

import jsonschema
import pytest
import yaml

from openeraseme.registry.loader import load_broker_yaml
from openeraseme.registry.schema import Broker


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
        from openeraseme.registry.schema import EmailOptOut

        yml = _repo_root() / "registry" / "brokers" / "eu" / "acxiom.yaml"
        broker = load_broker_yaml(yml)
        channel = broker.opt_out[0]
        assert isinstance(channel, EmailOptOut)
        assert "@" in channel.endpoint

    def test_web_form_opt_out_channel(self):
        from openeraseme.registry.schema import WebFormOptOut

        yml = _repo_root() / "registry" / "brokers" / "us" / "beenverified.yaml"
        broker = load_broker_yaml(yml)
        channel = broker.opt_out[0]
        assert isinstance(channel, WebFormOptOut)
        assert "steps" in channel.form_spec.model_dump()


class TestRegistryLoader:
    def test_load_all_brokers(self):
        from openeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers()
        assert len(brokers) >= 5
        assert all(isinstance(b, Broker) for b in brokers)

    def test_filter_by_jurisdiction(self):
        from openeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers(jurisdiction="DE")
        assert all("DE" in b.jurisdictions for b in brokers)

    def test_filter_by_priority(self):
        from openeraseme.registry.loader import load_all_brokers

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
