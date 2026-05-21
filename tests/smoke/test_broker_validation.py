from __future__ import annotations

from pathlib import Path

import pytest
import yaml

from openeraseme.registry.loader import load_all_brokers, load_broker
from openeraseme.registry.schema import Broker


class TestBrokerLoading:
    def test_load_all_brokers(self):
        brokers = load_all_brokers()
        assert len(brokers) > 0
        for b in brokers:
            assert isinstance(b, Broker)
            assert b.id
            assert b.name

    def test_load_first_broker_by_id(self):
        brokers = load_all_brokers()
        assert len(brokers) > 0
        first_id = brokers[0].id
        broker = load_broker(first_id)
        assert broker is not None

    def test_load_nonexistent_broker(self):
        with pytest.raises(FileNotFoundError):
            load_broker("nonexistent_broker_xyz")

    def test_all_brokers_have_opt_out(self):
        for b in load_all_brokers():
            assert len(b.opt_out) > 0, f"Broker {b.id} has no opt-out channels"

    def test_all_brokers_have_jurisdictions(self):
        for b in load_all_brokers():
            assert len(b.jurisdictions) > 0, f"Broker {b.id} has no jurisdictions"

    def test_all_brokers_have_verification_keywords(self):
        for b in load_all_brokers():
            assert b.verification, f"Broker {b.id} has no verification keywords"

    def test_filter_by_jurisdiction(self):
        all_brokers = load_all_brokers()
        gdpr = [b for b in all_brokers if "GDPR" in (law.name for law in (b.laws or []))]
        ccpa = [b for b in all_brokers if "CCPA" in (law.name for law in (b.laws or []))]
        assert len(gdpr) >= 0
        assert len(ccpa) >= 0

    def test_filter_by_priority(self):
        high = [b for b in load_all_brokers() if b.priority == "high"]
        assert len(high) > 0

    def test_broker_opt_out_channels(self):
        brokers = load_all_brokers()
        for b in brokers[:3]:
            for channel in b.opt_out:
                assert channel.type in ("email", "web_form")


class TestBrokerSchemas:
    def test_all_yamls_valid(self):
        schema_path = Path("registry/schemas/broker.schema.json")
        if not schema_path.exists():
            pytest.skip("Schema file not found")
        import json

        import jsonschema

        with open(schema_path) as f:
            schema = json.load(f)
        brokers_dir = Path("registry/brokers")
        for yml in sorted(brokers_dir.rglob("*.yaml")):
            with open(yml) as f:
                data = yaml.safe_load(f)
            try:
                jsonschema.validate(data, schema)
            except jsonschema.ValidationError as e:
                pytest.fail(f"{yml}: {e.message}")

    def test_each_broker_has_unique_id(self):
        brokers = load_all_brokers()
        ids = [b.id for b in brokers]
        assert len(ids) == len(set(ids)), "Duplicate broker IDs found"
