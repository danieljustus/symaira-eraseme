"""Tests for LLM consent scrubber."""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import patch

import pytest

from symeraseme.adapters.triage.scrubber import (
    grant_llm_consent,
    llm_consent_granted,
    revoke_llm_consent,
)


class TestLlmConsent:
    @pytest.fixture(autouse=True)
    def _clean_consent(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
        consent_file = tmp_path / ".llm_consent_granted"
        monkeypatch.setattr(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            lambda: consent_file,
        )
        monkeypatch.delenv("SYMERASEME_LLM_CONSENT", raising=False)
        consent_file.unlink(missing_ok=True)
        yield
        consent_file.unlink(missing_ok=True)

    def test_consent_not_granted_by_default(self) -> None:
        assert not llm_consent_granted()

    def test_consent_granted_via_env_var(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setenv("SYMERASEME_LLM_CONSENT", "1")
        assert llm_consent_granted()

    def test_grant_creates_file_with_metadata(self, tmp_path: Path) -> None:
        consent_file = tmp_path / ".llm_consent_granted"
        with patch(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            return_value=consent_file,
        ):
            grant_llm_consent()
            assert llm_consent_granted()
            assert consent_file.exists()
            data = json.loads(consent_file.read_text(encoding="utf-8"))
            assert data["granted"] is True
            assert data["scope"] == "llm_pii"
            assert "user" in data
            assert "granted_at" in data

    def test_grant_sets_restrictive_permissions(self, tmp_path: Path) -> None:
        consent_file = tmp_path / ".llm_consent_granted"
        with patch(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            return_value=consent_file,
        ):
            grant_llm_consent()
            perms = consent_file.stat().st_mode & 0o777
            assert perms == 0o600, f"Expected 0o600, got {oct(perms)}"

    def test_revoke_removes_file(self) -> None:
        grant_llm_consent()
        assert llm_consent_granted()

        revoke_llm_consent()
        assert not llm_consent_granted()

    def test_legacy_empty_file_treated_as_granted(self, tmp_path: Path) -> None:
        """Empty touch files from earlier versions remain valid."""
        consent_file = tmp_path / ".llm_consent_granted"
        with patch(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            return_value=consent_file,
        ):
            consent_file.parent.mkdir(parents=True, exist_ok=True)
            consent_file.write_text("")
            assert llm_consent_granted()

    def test_revoke_is_idempotent(self) -> None:
        revoke_llm_consent()
        assert not llm_consent_granted()
        revoke_llm_consent()
        assert not llm_consent_granted()

    def test_corrupt_json_file_denies_consent(self, tmp_path: Path) -> None:
        consent_file = tmp_path / ".llm_consent_granted"
        with patch(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            return_value=consent_file,
        ):
            consent_file.parent.mkdir(parents=True, exist_ok=True)
            consent_file.write_text("{invalid json", encoding="utf-8")
            assert not llm_consent_granted()

    def test_unreadable_file_denies_consent(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        consent_file = tmp_path / ".llm_consent_granted"
        with patch(
            "symeraseme.adapters.triage.scrubber._llm_consent_file",
            return_value=consent_file,
        ):
            consent_file.parent.mkdir(parents=True, exist_ok=True)
            consent_file.write_text('{"granted": true}', encoding="utf-8")
            original_read_text = Path.read_text

            def raise_os_error(self: Path, *args, **kwargs):
                if self == consent_file:
                    raise OSError("Permission denied")
                return original_read_text(self, *args, **kwargs)

            monkeypatch.setattr(Path, "read_text", raise_os_error)
            assert not llm_consent_granted()
