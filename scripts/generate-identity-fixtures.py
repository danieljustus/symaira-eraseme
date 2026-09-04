#!/usr/bin/env python3
"""Generate deterministic encrypted identity profile fixtures using python-final.

Uses the real archived Python implementation extracted from git tag 'python-final'
with fixed non-secret key, fixed non-secret nonce, and writes frozen fixtures and
provenance metadata into tests/fixtures/identity-contract/.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from datetime import date
from pathlib import Path


FIXED_KEY_HEX = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
FIXED_NONCE_HEX = "00112233445566778899aabb"


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    fixtures_dir = repo_root / "tests" / "fixtures" / "identity-contract"
    fixtures_dir.mkdir(parents=True, exist_ok=True)

    # 1. Extract real Python sources from git tag python-final
    temp_dir = tempfile.mkdtemp(prefix="eraseme-python-final-")
    try:
        archive_proc = subprocess.run(
            ["git", "archive", "python-final", "src/"],
            cwd=repo_root,
            capture_output=True,
            check=True,
        )
        subprocess.run(
            ["tar", "-x", "-C", temp_dir],
            input=archive_proc.stdout,
            check=True,
        )

        sys.path.insert(0, str(Path(temp_dir) / "src"))

        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        import symeraseme.core.identity as py_id
        from symeraseme.registry.schema import IdentityProfile, Address

        key = bytes.fromhex(FIXED_KEY_HEX)
        nonce = bytes.fromhex(FIXED_NONCE_HEX)
        aesgcm = AESGCM(key)

        profiles: dict[str, IdentityProfile] = {
            "minimal": IdentityProfile(
                full_name="Jane Doe",
                email_addresses=["jane@example.com"],
            ),
            "full": IdentityProfile(
                full_name="Jane Doe",
                name_variants=["Jane Roe", "Jane Smith"],
                date_of_birth=date(1990, 1, 15),
                addresses=[
                    Address(
                        street="123 Main St",
                        city="Berlin",
                        postal_code="10115",
                        country="DE",
                        state="Berlin",
                        valid_from=date(2020, 1, 1),
                        valid_to=None,
                    )
                ],
                email_addresses=["jane@example.com", "jane.doe@work.example.com"],
                phone_numbers=["+49-30-123456", "+1-555-0199"],
                jurisdictions=["DE", "EU", "US-CA"],
            ),
            "unicode": IdentityProfile(
                full_name="Jörg Müller",
                name_variants=["Jörg Mueller"],
                date_of_birth=date(1985, 12, 31),
                addresses=[
                    Address(
                        street="Goethestraße 42",
                        city="München",
                        postal_code="80331",
                        country="DE",
                        state=None,
                        valid_from=None,
                        valid_to=None,
                    )
                ],
                email_addresses=["joerg.mueller@example.de"],
                phone_numbers=["+49-89-9876543"],
                jurisdictions=["DE", "EU"],
            ),
        }

        # Resolve python-final commit hash for provenance
        commit_proc = subprocess.run(
            ["git", "rev-parse", "python-final"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=True,
        )
        python_final_commit = commit_proc.stdout.strip()

        provenance: dict[str, any] = {
            "generator": "scripts/generate-identity-fixtures.py",
            "python_final_tag": "python-final",
            "python_final_commit": python_final_commit,
            "key_hex": FIXED_KEY_HEX,
            "nonce_hex": FIXED_NONCE_HEX,
            "profiles": {},
        }

        for name, profile in profiles.items():
            header_bytes = json.dumps(
                {"version": 2, "nonce": FIXED_NONCE_HEX, "algorithm": "AES-256-GCM"},
            ).encode("utf-8")
            payload_str = profile.model_dump_json(indent=2)
            payload_bytes = payload_str.encode("utf-8")
            ciphertext = aesgcm.encrypt(nonce, payload_bytes, header_bytes)

            file_bytes = header_bytes + b"\n" + ciphertext

            enc_file = fixtures_dir / f"{name}.enc"
            enc_file.write_bytes(file_bytes)
            os.chmod(enc_file, 0o600)

            canonical_json_str = json.dumps(profile.model_dump(mode="json"), sort_keys=True)
            profile_hash = py_id.hash_profile(profile)

            # Verification round-trip through real Python decoder
            header_dec, _, ct_dec = file_bytes.partition(b"\n")
            h_obj = json.loads(header_dec)
            decrypted = aesgcm.decrypt(bytes.fromhex(h_obj["nonce"]), ct_dec, header_dec)
            reloaded = IdentityProfile.model_validate(json.loads(decrypted.decode("utf-8")))
            assert reloaded.full_name == profile.full_name, "Round-trip full_name mismatch"
            assert py_id.hash_profile(reloaded) == profile_hash, "Round-trip hash mismatch"

            provenance["profiles"][name] = {
                "file_enc": f"{name}.enc",
                "canonical_json": canonical_json_str,
                "canonical_hash": profile_hash,
                "header_json": header_bytes.decode("utf-8"),
                "ciphertext_hex": ciphertext.hex(),
                "payload_json": payload_str,
            }
            print(f"Generated fixture {name}: hash={profile_hash}")

        manifest_file = fixtures_dir / "provenance.json"
        manifest_file.write_text(json.dumps(provenance, indent=2) + "\n")
        print(f"Wrote provenance to {manifest_file}")

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
