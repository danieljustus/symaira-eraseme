from __future__ import annotations

import contextlib
import json
import logging
import os
from pathlib import Path

import keyring
from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from symeraseme.core.config import get_config
from symeraseme.registry.schema import IdentityProfile

logger = logging.getLogger(__name__)

_PROFILE_CACHE: dict[str, IdentityProfile] = {}

SERVICE_NAME = "symeraseme"
KEYRING_USERNAME = "identity-master-key"
KEY_LENGTH = 32  # AES-256
NONCE_LENGTH = 12  # AES-GCM standard
DEFAULT_PROFILE_PATH = "~/.config/symeraseme/identity.enc"


def _profile_path(path: str | None = None) -> Path:
    if path:
        return Path(path).expanduser().resolve()
    return get_config().identity_path


def _get_existing_master_key() -> bytes:
    """Return the stored master key or raise RuntimeError if absent.

    Use this in read/decrypt paths so a missing keyring entry fails
    fast instead of silently minting a new key that cannot decrypt.
    """
    stored = keyring.get_password(SERVICE_NAME, KEYRING_USERNAME)
    if stored:
        return bytes.fromhex(stored)
    msg = (
        "Identity master key not found in system keyring. "
        "Run 'symeraseme init-profile' to create a profile and key."
    )
    raise RuntimeError(msg)


def _get_or_create_master_key() -> bytes:
    """Return the stored master key, creating one if absent.

    Use this in write/encrypt paths (save_profile, first-time setup).
    """
    stored = keyring.get_password(SERVICE_NAME, KEYRING_USERNAME)
    if stored:
        return bytes.fromhex(stored)

    key = AESGCM.generate_key(bit_length=KEY_LENGTH * 8)
    keyring.set_password(SERVICE_NAME, KEYRING_USERNAME, key.hex())
    return key


def _delete_master_key() -> None:
    with contextlib.suppress(keyring.errors.PasswordDeleteError):
        keyring.delete_password(SERVICE_NAME, KEYRING_USERNAME)


def save_profile(
    profile: IdentityProfile,
    path: str | None = None,
) -> Path:
    target = _profile_path(path)
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)

    key = _get_or_create_master_key()
    aesgcm = AESGCM(key)
    nonce = os.urandom(NONCE_LENGTH)

    header = json.dumps(
        {"version": 2, "nonce": nonce.hex(), "algorithm": "AES-256-GCM"},
    ).encode("utf-8")

    payload = profile.model_dump_json(indent=2).encode("utf-8")
    ciphertext = aesgcm.encrypt(nonce, payload, header)

    with open(target, "wb") as f:
        f.write(header + b"\n" + ciphertext)
    os.chmod(target, 0o600)

    # Invalidate cache so the next load reads the fresh file.
    _PROFILE_CACHE.pop(str(target), None)

    return target


def load_profile(path: str | None = None) -> IdentityProfile:
    target = _profile_path(path)
    cache_key = str(target)
    if cache_key in _PROFILE_CACHE:
        return _PROFILE_CACHE[cache_key]

    if not target.exists():
        logger.debug("Identity profile not found at %s", target)
        msg = "Identity profile not found. Run 'symeraseme init-profile' first."
        raise FileNotFoundError(msg)

    key = _get_existing_master_key()
    aesgcm = AESGCM(key)

    with open(target, "rb") as f:
        raw = f.read()

    header_bytes, _, ciphertext = raw.partition(b"\n")
    header = json.loads(header_bytes)
    nonce = bytes.fromhex(header["nonce"])

    try:
        plaintext = aesgcm.decrypt(nonce, ciphertext, header_bytes)
    except InvalidTag:
        version = header.get("version", 0)
        if version == 0:
            logger.warning(
                "AAD verification failed — retrying without AAD for legacy file. "
                "Re-encrypt the profile to upgrade: symeraseme save-profile",
            )
            plaintext = aesgcm.decrypt(nonce, ciphertext, None)
        else:
            raise
    data = json.loads(plaintext.decode("utf-8"))

    profile = IdentityProfile.model_validate(data)
    _PROFILE_CACHE[cache_key] = profile
    return profile


def delete_profile(path: str | None = None) -> None:
    target = _profile_path(path)
    if target.exists():
        target.unlink()
    _delete_master_key()


def profile_exists(path: str | None = None) -> bool:
    return _profile_path(path).exists()


def hash_profile(profile: IdentityProfile) -> str:
    """Return a deterministic, non-reversible hash of the profile contents.

    Used for audit trails to prove which identity profile version was used
    for a request without leaking the profile itself.
    """
    import hashlib
    import json

    canonical = json.dumps(profile.model_dump(mode="json"), sort_keys=True)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()
