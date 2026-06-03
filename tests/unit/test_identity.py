"""Tests for the identity vault (AES-GCM + keyring)."""

import json
import os
from pathlib import Path
from unittest.mock import patch

import pytest

from symeraseme.registry.schema import IdentityProfile


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
            patch("symeraseme.core.identity.keyring.set_password", fake_set_password),
            patch("symeraseme.core.identity.keyring.get_password", fake_get_password),
            patch("symeraseme.core.identity.keyring.delete_password", fake_delete_password),
        ):
            yield

    def test_save_and_load_profile(self, monkeypatch):
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_identity.enc")
        Path("/tmp/symeraseme_test_identity.enc").unlink(missing_ok=True)

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
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_encrypted.enc")
        Path("/tmp/symeraseme_test_encrypted.enc").unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Secret User", email_addresses=["secret@example.com"])
        path = vault.save_profile(profile)

        with open(path, "rb") as f:
            raw = f.read()

        assert b"Secret User" not in raw

        vault.delete_profile()

    def test_roundtrip_preserves_all_data(self, monkeypatch):
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_roundtrip.enc")
        Path("/tmp/symeraseme_test_roundtrip.enc").unlink(missing_ok=True)

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
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_nonexistent.enc")
        Path("/tmp/symeraseme_test_nonexistent.enc").unlink(missing_ok=True)

        with pytest.raises(FileNotFoundError):
            vault.load_profile()

    def test_tampered_ciphertext_fails_closed(self, monkeypatch):
        from cryptography.exceptions import InvalidTag

        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_tampered.enc")
        path = Path("/tmp/symeraseme_test_tampered.enc")
        path.unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Test User", email_addresses=["test@example.com"])
        vault.save_profile(profile)

        raw = path.read_bytes()
        header_bytes, _, ciphertext = raw.partition(b"\n")
        tampered = bytearray(ciphertext)
        tampered[-1] ^= 0xFF
        path.write_bytes(header_bytes + b"\n" + bytes(tampered))

        with pytest.raises(InvalidTag):
            vault.load_profile()

        vault.delete_profile()

    def test_load_with_missing_key_fails_fast(self, monkeypatch):
        """A missing keyring entry on load must not mint a new key."""
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_missing_key.enc")
        path = Path("/tmp/symeraseme_test_missing_key.enc")
        path.unlink(missing_ok=True)

        # Save with a key, then delete the key
        profile = IdentityProfile(full_name="Test User", email_addresses=["test@example.com"])
        vault.save_profile(profile)
        vault._delete_master_key()

        with pytest.raises(RuntimeError, match="master key not found"):
            vault.load_profile()

        vault.delete_profile()

    def test_save_creates_key_when_missing(self, monkeypatch):
        """Save path must auto-create a missing master key."""
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_save_create.enc")
        path = Path("/tmp/symeraseme_test_save_create.enc")
        path.unlink(missing_ok=True)

        # Ensure no key exists
        vault._delete_master_key()

        profile = IdentityProfile(full_name="Test User", email_addresses=["test@example.com"])
        vault.save_profile(profile)

        assert vault.profile_exists()
        assert vault.load_profile().full_name == "Test User"

        vault.delete_profile()

    def test_legacy_version_0_fallback(self, monkeypatch):
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM

        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_legacy.enc")
        path = Path("/tmp/symeraseme_test_legacy.enc")
        path.unlink(missing_ok=True)

        key = AESGCM.generate_key(bit_length=256)
        aesgcm = AESGCM(key)
        nonce = os.urandom(12)
        profile = IdentityProfile(full_name="Legacy User", email_addresses=["legacy@example.com"])
        payload = profile.model_dump_json(indent=2).encode("utf-8")
        ciphertext = aesgcm.encrypt(nonce, payload, None)
        header = json.dumps(
            {"version": 0, "nonce": nonce.hex(), "algorithm": "AES-256-GCM"},
        ).encode("utf-8")
        path.write_bytes(header + b"\n" + ciphertext)

        with patch("symeraseme.core.identity.keyring.get_password", return_value=key.hex()):
            loaded = vault.load_profile()

        assert loaded.full_name == "Legacy User"

        vault.delete_profile()

    def test_load_uses_cache_after_first_call(self, monkeypatch):
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_cache.enc")
        path = Path("/tmp/symeraseme_test_cache.enc")
        path.unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Cached User", email_addresses=["cache@example.com"])
        vault.save_profile(profile)

        vault._PROFILE_CACHE.clear()
        loaded1 = vault.load_profile()
        loaded2 = vault.load_profile()
        assert loaded1 is loaded2
        assert len(vault._PROFILE_CACHE) == 1

        vault.delete_profile()

    def test_save_profile_sets_restrictive_permissions(self, monkeypatch, tmp_path):
        """Saved identity file must be 0o600 and parent directory 0o700."""
        import symeraseme.core.identity as vault

        identity_path = tmp_path / "identity.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(identity_path))

        profile = IdentityProfile(full_name="Test User", email_addresses=["test@example.com"])
        vault.save_profile(profile)

        file_perms = identity_path.stat().st_mode & 0o777
        assert file_perms == 0o600, f"Expected 0o600, got {oct(file_perms)}"

        dir_perms = identity_path.parent.stat().st_mode & 0o777
        assert dir_perms == 0o700, f"Expected 0o700, got {oct(dir_perms)}"

        vault.delete_profile()

    def test_save_invalidates_cache(self, monkeypatch):
        import symeraseme.core.identity as vault

        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", "/tmp/symeraseme_test_inval.enc")
        path = Path("/tmp/symeraseme_test_inval.enc")
        path.unlink(missing_ok=True)

        profile = IdentityProfile(full_name="Original", email_addresses=["orig@example.com"])
        vault.save_profile(profile)

        vault._PROFILE_CACHE.clear()
        loaded1 = vault.load_profile()

        updated = IdentityProfile(full_name="Updated", email_addresses=["upd@example.com"])
        vault.save_profile(updated)

        loaded2 = vault.load_profile()
        assert loaded2.full_name == "Updated"
        assert loaded1 is not loaded2

        vault.delete_profile()
