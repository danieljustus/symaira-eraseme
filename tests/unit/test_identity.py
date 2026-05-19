"""Tests for the identity vault (AES-GCM + keyring)."""

from pathlib import Path
from unittest.mock import patch

import pytest

from openeraseme.registry.schema import IdentityProfile


class TestIdentityProfile:
    def test_minimal_profile(self):
        profile = IdentityProfile(full_name="Jane Doe", email_addresses=["jane@example.com"])
        assert profile.full_name == "Jane Doe"
        assert profile.email_addresses == ["jane@example.com"]

    def test_profile_with_all_fields(self):
        from datetime import date

        profile = IdentityProfile(
            full_name="Jane Doe",
            name_variants=["Jane Roe", "Jane Smith"],
            date_of_birth=date(1990, 1, 15),
            addresses=[
                {"street": "123 Main St", "city": "Berlin", "postal_code": "10115", "country": "DE"}
            ],
            email_addresses=["jane@example.com"],
            phone_numbers=["+49-30-123456"],
            jurisdictions=["DE", "EU"],
        )
        assert len(profile.addresses) == 1
        assert profile.jurisdictions == ["DE", "EU"]

    def test_serialize_deserialize_roundtrip(self):
        profile = IdentityProfile(full_name="Alice", email_addresses=["a@b.com"])
        data = profile.model_dump()
        restored = IdentityProfile.model_validate(data)
        assert restored.full_name == profile.full_name
        assert restored.email_addresses == profile.email_addresses


class TestIdentityVault:
    @pytest.fixture(autouse=True)
    def _fake_keyring(self):
        fake_store: dict[str, str] = {}

        def fake_set_password(service, username, password):
            fake_store[f"{service}:{username}"] = password

        def fake_get_password(service, username):
            return fake_store.get(f"{service}:{username}")

        def fake_delete_password(service, username):
            fake_store.pop(f"{service}:{username}", None)

        with (
            patch("openeraseme.core.identity.keyring.set_password", fake_set_password),
            patch("openeraseme.core.identity.keyring.get_password", fake_get_password),
            patch("openeraseme.core.identity.keyring.delete_password", fake_delete_password),
        ):
            yield

    def test_save_and_load_profile(self, monkeypatch):
        import openeraseme.core.identity as vault

        monkeypatch.setenv("OPENERASEME_IDENTITY_PATH", "/tmp/openeraseme_test_identity.enc")
        Path("/tmp/openeraseme_test_identity.enc").unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Test User", email_addresses=["test@example.com"])
        path = vault.save_profile(profile)
        assert path.exists()
        assert vault.profile_exists()

        loaded = vault.load_profile()
        assert loaded.full_name == "Test User"
        assert loaded.email_addresses == ["test@example.com"]

        vault.delete_profile()
        assert not vault.profile_exists()

    def test_encrypted_file_is_not_plaintext(self, monkeypatch):
        import openeraseme.core.identity as vault

        monkeypatch.setenv("OPENERASEME_IDENTITY_PATH", "/tmp/openeraseme_test_encrypted.enc")
        Path("/tmp/openeraseme_test_encrypted.enc").unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Secret User", email_addresses=["secret@example.com"])
        path = vault.save_profile(profile)

        with open(path, "rb") as f:
            raw = f.read()

        assert b"Secret User" not in raw

        vault.delete_profile()

    def test_roundtrip_preserves_all_data(self, monkeypatch):
        import openeraseme.core.identity as vault

        monkeypatch.setenv("OPENERASEME_IDENTITY_PATH", "/tmp/openeraseme_test_roundtrip.enc")
        Path("/tmp/openeraseme_test_roundtrip.enc").unlink(missing_ok=True)

        original = IdentityProfile(
            full_name="Jane Doe",
            name_variants=["Jane Roe"],
            email_addresses=["jane@example.com"],
            phone_numbers=["+1-555-1234"],
            jurisdictions=["US", "EU"],
        )
        vault.save_profile(original)
        loaded = vault.load_profile()
        assert loaded.model_dump() == original.model_dump()

        vault.delete_profile()

    def test_load_nonexistent_raises(self, monkeypatch):
        import openeraseme.core.identity as vault

        monkeypatch.setenv("OPENERASEME_IDENTITY_PATH", "/tmp/openeraseme_test_nonexistent.enc")
        Path("/tmp/openeraseme_test_nonexistent.enc").unlink(missing_ok=True)

        with pytest.raises(FileNotFoundError):
            vault.load_profile()
