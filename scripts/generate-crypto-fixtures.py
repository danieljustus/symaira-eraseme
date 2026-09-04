#!/usr/bin/env python3
"""Generate deterministic V1/V2/V3 encrypted database fixtures through the real
archived python-final implementation.

This script extracts src/symeraseme/core/db_encryption.py from the python-final
git tag and executes its encryption routines with fixed, non-secret parameters
(master key, salts, timestamp, and IV) to generate reproducible test fixtures.

Usage:
    python3 scripts/generate-crypto-fixtures.py
"""

from __future__ import annotations

import hashlib
import json
import os
import struct
import subprocess
import sys
import types
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = REPO_ROOT / "tests" / "fixtures" / "event-store" / "crypto"
GOLDEN_DB = REPO_ROOT / "tests" / "fixtures" / "event-store" / "golden-campaign.db"

# Fixed, non-secret cryptographic test parameters
FIXED_MASTER_KEY = b"symaira-eraseme-golden-master-32"  # 32 bytes
FIXED_TIMESTAMP = 1756713900  # 2025-09-01T08:05:00Z
FIXED_IV = b"0123456789abcdef"  # 16 bytes
V1_SALT = b"symeraseme-db-encryption-v1"  # 27 bytes (fixed PBKDF2 salt)
V2_SALT = b"salt-sixteen-v2!"  # 16 bytes
V3_SALT = b"salt-sixteen-v3!"  # 16 bytes
LEGACY_NONCE = b"0123456789ab"  # 12 bytes nonce for accidental Go AES-GCM

SMALL_PLAINTEXT = b"SQLite format 3\x00deterministic test payload for small vector verification."


def load_archived_db_encryption() -> tuple[types.ModuleType, str]:
    """Extract and compile db_encryption.py from python-final tag."""
    commit_hash = (
        subprocess.check_output(["git", "rev-parse", "python-final"], cwd=REPO_ROOT)
        .decode("utf-8")
        .strip()
    )
    code = subprocess.check_output(
        ["git", "show", "python-final:src/symeraseme/core/db_encryption.py"],
        cwd=REPO_ROOT,
    ).decode("utf-8")

    mod = types.ModuleType("db_encryption")
    identity_mod = types.ModuleType("symeraseme.core.identity")
    identity_mod._get_existing_master_key = lambda: FIXED_MASTER_KEY
    sys.modules["symeraseme.core.identity"] = identity_mod
    sys.modules["symeraseme.core.db_encryption"] = mod
    exec(code, mod.__dict__)

    return mod, commit_hash


def generate_legacy_go_token(plaintext: bytes, key: bytes, timestamp: int, nonce: bytes) -> bytes:
    """Generate the accidental Go AES-256-GCM + outer HMAC token layout.

    Layout: 0x80 (1 byte) || timestamp (8 bytes, uint64 BE) || nonce (12 bytes) ||
            AES-256-GCM ciphertext+tag || 32-byte HMAC-SHA256(key, frame).
    """
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    import hmac

    aesgcm = AESGCM(key)
    ct = aesgcm.encrypt(nonce, plaintext, None)

    body = bytearray()
    body.append(0x80)
    body.extend(struct.pack(">Q", timestamp))
    body.extend(nonce)
    body.extend(ct)

    h = hmac.new(key, bytes(body), hashlib.sha256)
    sig = h.digest()
    body.extend(sig)
    return bytes(body)


def main() -> None:
    mod, commit_hash = load_archived_db_encryption()
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    plain_db_bytes = GOLDEN_DB.read_bytes()
    plain_db_hash = hashlib.sha256(plain_db_bytes).hexdigest()

    # Monkeypatch Fernet._encrypt_from_parts on the archived module
    orig_encrypt_from_parts = mod.Fernet._encrypt_from_parts

    def deterministic_encrypt_from_parts(self, data: bytes, current_time: int = 0, iv: bytes = b"") -> bytes:
        return orig_encrypt_from_parts(self, data, FIXED_TIMESTAMP, FIXED_IV)

    mod.Fernet._encrypt_from_parts = deterministic_encrypt_from_parts

    # 1. Python V1: ENC_HEADER_V1 + standard Fernet token
    k1 = mod._get_db_fernet_key(salt=None, version=1)
    f1 = mod.Fernet(k1)
    t1 = f1._encrypt_from_parts(plain_db_bytes, FIXED_TIMESTAMP, FIXED_IV)
    v1_bytes = mod.ENC_HEADER_V1 + t1
    v1_file = OUTPUT_DIR / "golden-campaign-v1-python.db"
    v1_file.write_bytes(v1_bytes)

    # 2. Python V2: ENC_MAGIC_V2 + 16-byte salt + standard Fernet token
    k2 = mod._get_db_fernet_key(salt=V2_SALT, version=2)
    f2 = mod.Fernet(k2)
    t2 = f2._encrypt_from_parts(plain_db_bytes, FIXED_TIMESTAMP, FIXED_IV)
    v2_bytes = mod.ENC_MAGIC_V2 + V2_SALT + t2
    v2_file = OUTPUT_DIR / "golden-campaign-v2-python.db"
    v2_file.write_bytes(v2_bytes)

    # 3. Python V3: ENC_MAGIC_V3 + 16-byte salt + standard Fernet token
    k3 = mod._get_db_fernet_key(salt=V3_SALT, version=3)
    f3 = mod.Fernet(k3)
    t3 = f3._encrypt_from_parts(plain_db_bytes, FIXED_TIMESTAMP, FIXED_IV)
    v3_bytes = mod.ENC_MAGIC_V3 + V3_SALT + t3
    v3_file = OUTPUT_DIR / "golden-campaign-v3-python.db"
    v3_file.write_bytes(v3_bytes)

    # Small vector Python V3
    small_t3 = f3._encrypt_from_parts(SMALL_PLAINTEXT, FIXED_TIMESTAMP, FIXED_IV)
    small_v3_bytes = mod.ENC_MAGIC_V3 + V3_SALT + small_t3
    small_v3_file = OUTPUT_DIR / "small-vector-v3-python.db"
    small_v3_file.write_bytes(small_v3_bytes)

    # 4. Accidental legacy Go V1/V2/V3 payloads for backward compatibility and migration tests
    from base64 import urlsafe_b64decode
    # Derive raw keys
    raw_k1 = urlsafe_b64decode(k1)
    raw_k2 = urlsafe_b64decode(k2)
    raw_k3 = urlsafe_b64decode(k3)

    legacy_tok1 = generate_legacy_go_token(plain_db_bytes, raw_k1, FIXED_TIMESTAMP, LEGACY_NONCE)
    legacy_v1_bytes = mod.ENC_HEADER_V1 + legacy_tok1
    legacy_v1_file = OUTPUT_DIR / "golden-campaign-v1-legacy-go.db"
    legacy_v1_file.write_bytes(legacy_v1_bytes)

    legacy_tok2 = generate_legacy_go_token(plain_db_bytes, raw_k2, FIXED_TIMESTAMP, LEGACY_NONCE)
    legacy_v2_bytes = mod.ENC_MAGIC_V2 + V2_SALT + legacy_tok2
    legacy_v2_file = OUTPUT_DIR / "golden-campaign-v2-legacy-go.db"
    legacy_v2_file.write_bytes(legacy_v2_bytes)

    legacy_tok3 = generate_legacy_go_token(plain_db_bytes, raw_k3, FIXED_TIMESTAMP, LEGACY_NONCE)
    legacy_v3_bytes = mod.ENC_MAGIC_V3 + V3_SALT + legacy_tok3
    legacy_v3_file = OUTPUT_DIR / "golden-campaign-v3-legacy-go.db"
    legacy_v3_file.write_bytes(legacy_v3_bytes)

    # Small vector legacy Go V3
    small_legacy_tok3 = generate_legacy_go_token(SMALL_PLAINTEXT, raw_k3, FIXED_TIMESTAMP, LEGACY_NONCE)
    small_legacy_v3_bytes = mod.ENC_MAGIC_V3 + V3_SALT + small_legacy_tok3
    small_legacy_v3_file = OUTPUT_DIR / "small-vector-v3-legacy-go.db"
    small_legacy_v3_file.write_bytes(small_legacy_v3_bytes)

    # Verify python-final decrypts the Python files
    for path, expected_plain in [
        (v1_file, plain_db_bytes),
        (v2_file, plain_db_bytes),
        (v3_file, plain_db_bytes),
        (small_v3_file, SMALL_PLAINTEXT),
    ]:
        decrypted_tmp = mod._decrypt_to_temp(path)
        try:
            assert decrypted_tmp.read_bytes() == expected_plain, f"Verification failed for {path}"
        finally:
            decrypted_tmp.unlink(missing_ok=True)

    # Write provenance record
    provenance = {
        "generator": "scripts/generate-crypto-fixtures.py",
        "python_final_commit": commit_hash,
        "golden_campaign_sha256": plain_db_hash,
        "golden_campaign_size": len(plain_db_bytes),
        "small_plaintext_sha256": hashlib.sha256(SMALL_PLAINTEXT).hexdigest(),
        "small_plaintext_size": len(SMALL_PLAINTEXT),
        "parameters": {
            "master_key_hex": FIXED_MASTER_KEY.hex(),
            "master_key_ascii": FIXED_MASTER_KEY.decode("ascii"),
            "fixed_timestamp": FIXED_TIMESTAMP,
            "fixed_iv_hex": FIXED_IV.hex(),
            "v1_salt_hex": V1_SALT.hex(),
            "v2_salt_hex": V2_SALT.hex(),
            "v3_salt_hex": V3_SALT.hex(),
            "legacy_nonce_hex": LEGACY_NONCE.hex(),
        },
        "fixtures": {
            "golden-campaign-v1-python.db": {
                "sha256": hashlib.sha256(v1_bytes).hexdigest(),
                "size": len(v1_bytes),
                "format": "standard-fernet",
                "version": 1,
            },
            "golden-campaign-v2-python.db": {
                "sha256": hashlib.sha256(v2_bytes).hexdigest(),
                "size": len(v2_bytes),
                "format": "standard-fernet",
                "version": 2,
            },
            "golden-campaign-v3-python.db": {
                "sha256": hashlib.sha256(v3_bytes).hexdigest(),
                "size": len(v3_bytes),
                "format": "standard-fernet",
                "version": 3,
            },
            "small-vector-v3-python.db": {
                "sha256": hashlib.sha256(small_v3_bytes).hexdigest(),
                "size": len(small_v3_bytes),
                "format": "standard-fernet",
                "version": 3,
            },
            "golden-campaign-v1-legacy-go.db": {
                "sha256": hashlib.sha256(legacy_v1_bytes).hexdigest(),
                "size": len(legacy_v1_bytes),
                "format": "legacy-go-aes-gcm",
                "version": 1,
            },
            "golden-campaign-v2-legacy-go.db": {
                "sha256": hashlib.sha256(legacy_v2_bytes).hexdigest(),
                "size": len(legacy_v2_bytes),
                "format": "legacy-go-aes-gcm",
                "version": 2,
            },
            "golden-campaign-v3-legacy-go.db": {
                "sha256": hashlib.sha256(legacy_v3_bytes).hexdigest(),
                "size": len(legacy_v3_bytes),
                "format": "legacy-go-aes-gcm",
                "version": 3,
            },
            "small-vector-v3-legacy-go.db": {
                "sha256": hashlib.sha256(small_legacy_v3_bytes).hexdigest(),
                "size": len(small_legacy_v3_bytes),
                "format": "legacy-go-aes-gcm",
                "version": 3,
            },
        },
    }

    provenance_path = OUTPUT_DIR / "provenance.json"
    with open(provenance_path, "w", encoding="utf-8") as f:
        json.dump(provenance, f, indent=2)

    print("All fixtures generated and verified successfully.")
    print(f"Provenance recorded at {provenance_path}")


if __name__ == "__main__":
    main()
