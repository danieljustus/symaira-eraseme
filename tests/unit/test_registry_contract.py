"""Registry data-contract conformance tests (issue #707).

These tests make the versioned broker contract executable: the golden
fixtures under tests/fixtures/registry-contract/ exercise every loader
branch and must pass through the Python loader unchanged, the invalid
fixture must be rejected, and the full live registry must validate
against the pinned schema version.
"""

from __future__ import annotations

import json

import jsonschema
import pytest
import yaml

from symeraseme.registry.loader import _load_broker_schema, load_broker_yaml

FIXTURE_DIR = "tests/fixtures/registry-contract"
VALID_FIXTURES = [
    "golden-webform-us.yaml",
    "golden-email-eu.yaml",
    "golden-multi-uk.yaml",
    "golden-minimal-us.yaml",
]
INVALID_FIXTURES = ["invalid-unknown-field.yaml"]


def test_manifest_and_schema_agree_on_schema_version():
    with open("registry/manifest.json") as f:
        manifest = json.load(f)
    schema = _load_broker_schema()
    assert manifest["schema_version"] == 1
    assert schema["schema_version"] == manifest["schema_version"]
    assert manifest["schemas"]["broker"] == "schemas/broker.schema.json"


@pytest.mark.parametrize("fixture", VALID_FIXTURES)
def test_golden_fixture_loads_through_python_loader(fixture):
    broker = load_broker_yaml(f"{FIXTURE_DIR}/{fixture}")
    # The loader produced a structured model, not a raw dict.
    assert broker.id.startswith("golden-") or broker.id.startswith("golden")
    assert broker.opt_out
    assert broker.jurisdictions


@pytest.mark.parametrize("fixture", INVALID_FIXTURES)
def test_invalid_fixture_rejected_by_schema_and_loader(fixture):
    with open(f"{FIXTURE_DIR}/{fixture}") as f:
        data = yaml.safe_load(f)
    schema = _load_broker_schema()
    with pytest.raises(jsonschema.ValidationError, match="Additional properties|is not one of"):
        jsonschema.validate(data, schema)
    with pytest.raises(jsonschema.ValidationError, match="Additional properties|is not one of"):
        load_broker_yaml(f"{FIXTURE_DIR}/{fixture}")


def test_every_live_broker_document_validates_against_pinned_schema():
    """Full-registry conformance run: all broker YAMLs must validate.

    This is the Python-side mirror of the CI `schema-validate` job so the
    Go loader can diff against the same assertion later.
    """
    from pathlib import Path

    from symeraseme.registry.loader import _broker_validator

    validator = _broker_validator()
    files = sorted(Path("registry/brokers").rglob("*.yaml"))
    files = [p for p in files if not p.name.startswith("_")]
    assert len(files) >= 1000, "registry unexpectedly small"
    for path in files:
        with open(path) as f:
            data = yaml.safe_load(f)
        try:
            validator.validate(data)
        except jsonschema.ValidationError as e:
            pytest.fail(f"{path}: {e.message} @ {list(e.path)}")
        assert data["id"] == path.stem, f"id/stem mismatch in {path}"
