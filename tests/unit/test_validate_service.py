"""Tests for the registry validation service in ``services/validate.py``."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest


class TestHandleValidate:
    """Coverage for ``handle_validate`` — all validation paths."""

    def _import_mod(self):
        import symeraseme.services.validate as m

        return m

    def test_empty_registry_returns_success(self, tmp_path):
        """No YAML files means empty summary with ok=True."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.success is True
        assert result.data["totals"]["checked"] == 0

    def test_skips_underscore_prefixed_files(self, tmp_path):
        """Files starting with ``_`` should be skipped."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        (brokers_dir / "_private.yaml").write_text("name: test")
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["checked"] == 0

    def test_yaml_parse_error_adds_failure(self, tmp_path):
        """Invalid YAML should be caught at the parse stage."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        (brokers_dir / "bad.yaml").write_text("{unclosed: [")
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["failed"] == 1
        assert result.data["failed"][0]["stage"] == "yaml"
        assert result.success is False

    def test_json_schema_validation_error_adds_failure(self, tmp_path):
        """Data that fails JSON Schema validation should be reported."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        (brokers_dir / "bad.yaml").write_text("id: missing-all-required-fields\n")
        mod = self._import_mod()

        schema = {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value=schema),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["failed"] == 1
        assert result.data["failed"][0]["stage"] == "schema"
        assert result.success is False

    def test_pydantic_validation_error_adds_failure(self, tmp_path):
        """Data that passes JSON Schema but fails Pydantic model_validate."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        (brokers_dir / "bad.yaml").write_text("id: test-broker\nname: Test\n")
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
            patch.object(mod, "Broker") as mock_broker_class,
        ):
            mock_broker_class.model_validate.side_effect = ValueError("invalid model")
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["failed"] == 1
        assert result.data["failed"][0]["stage"] == "pydantic"
        assert result.success is False

    def test_valid_broker_adds_to_valid_list(self, tmp_path):
        """A fully valid broker should appear in the valid list."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        yaml_content = """\
id: test-broker
name: Test Broker
website: https://example.com
category: people-search
jurisdictions:
  - US
laws:
  - CCPA
priority: high
opt_out:
  - type: email
    endpoint: test@example.com
    template: test
    locale: en
    required_fields: []
    expected_response_days: 30
disabled: false
"""
        (brokers_dir / "test-broker.yaml").write_text(yaml_content)
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["valid"] == 1
        assert result.data["totals"]["failed"] == 0
        assert result.data["totals"]["checked"] == 1
        assert result.success is True

    def test_duplicate_id_detection(self, tmp_path):
        """Two files with the same broker id should be flagged."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        yaml_content = """id: dup-broker
name: Duplicate
website: https://example.com
category: people-search
jurisdictions:
  - US
laws:
  - CCPA
priority: high
opt_out:
  - type: email
    endpoint: a@b.com
    template: t
    locale: en
    required_fields: []
    expected_response_days: 30
disabled: false
"""
        (brokers_dir / "dup1.yaml").write_text(yaml_content)
        (brokers_dir / "dup2.yaml").write_text(yaml_content)
        mod = self._import_mod()
        from pydantic import BaseModel

        class FakeBroker(BaseModel):
            id: str
            disabled: bool = False

        def fake_model_validate(data, **kwargs):
            return FakeBroker(id="dup-broker", disabled=False)

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
            patch.object(mod, "Broker") as mock_broker_class,
        ):
            mock_broker_class.model_validate.side_effect = fake_model_validate
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["duplicate_ids"] == 1
        assert len(result.data["duplicate_ids"]) == 1
        assert result.data["duplicate_ids"][0]["id"] == "dup-broker"
        assert result.success is False

    def test_mixed_results(self, tmp_path):
        """Mix of valid, invalid entries."""
        brokers_dir = tmp_path / "brokers"
        brokers_dir.mkdir(parents=True)
        (brokers_dir / "bad-yaml.yaml").write_text("{unclosed: [bad")
        valid_yaml = """\
id: valid-one
name: Valid One
website: https://example.com
category: people-search
jurisdictions:
  - US
laws:
  - CCPA
priority: high
opt_out:
  - type: email
    endpoint: a@b.com
    template: t
    locale: en
    required_fields: []
    expected_response_days: 30
disabled: false
"""
        (brokers_dir / "valid-one.yaml").write_text(valid_yaml)
        mod = self._import_mod()

        with (
            patch.object(mod, "_registry_dir", return_value=tmp_path),
            patch.object(mod, "broker_schema", return_value={"type": "object"}),
        ):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["totals"]["checked"] == 2
        assert result.data["totals"]["valid"] == 1
        assert result.data["totals"]["failed"] == 1

    def test_custom_registry_dir_overrides_default(self, tmp_path):
        """Passing an explicit registry_dir should use that instead of _registry_dir()."""
        brokers_dir = tmp_path / "custom_brokers"
        brokers_dir.mkdir(parents=True)
        mod = self._import_mod()

        with patch.object(mod, "broker_schema", return_value={"type": "object"}):
            result = mod.handle_validate(registry_dir=str(brokers_dir))

        assert result.data["registry_dir"] == str(brokers_dir)
