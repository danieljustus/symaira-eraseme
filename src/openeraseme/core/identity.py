from __future__ import annotations

import contextlib
import json
import logging
import os
from pathlib import Path

import keyring
from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from openeraseme.registry.schema import IdentityProfile

logger = logging.getLogger(__name__)

SERVICE_NAME = "openeraseme"
KEYRING_USERNAME = "identity-master-key"
KEY_LENGTH = 32  # AES-256
NONCE_LENGTH = 12  # AES-GCM standard
DEFAULT_PROFILE_PATH = "~/.config/openeraseme/identity.enc"


def _profile_path(path: str | None = None) -> Path:
    raw = path or os.environ.get("OPENERASEME_IDENTITY_PATH") or DEFAULT_PROFILE_PATH
    return Path(raw).expanduser().resolve()


def _get_or_create_master_key() -> bytes:
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
    target.parent.mkdir(parents=True, exist_ok=True)

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

    return target


def load_profile(path: str | None = None) -> IdentityProfile:
    target = _profile_path(path)
    if not target.exists():
        msg = f"Identity profile not found at {target}. Run 'openeraseme init-profile' first."
        raise FileNotFoundError(msg)

    key = _get_or_create_master_key()
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
                "Re-encrypt the profile to upgrade: openeraseme save-profile",
            )
            plaintext = aesgcm.decrypt(nonce, ciphertext, None)
        else:
            raise
    data = json.loads(plaintext.decode("utf-8"))

    return IdentityProfile.model_validate(data)


def delete_profile(path: str | None = None) -> None:
    target = _profile_path(path)
    if target.exists():
        target.unlink()
    _delete_master_key()


def profile_exists(path: str | None = None) -> bool:
    return _profile_path(path).exists()
