#!/usr/bin/env python3
"""Disposable encrypted-store rollback bridge; local evidence, never cutover."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import sqlite3

import plain_store_switchback as gate


FIXTURE = gate.REPO / "tests/fixtures/event-store/crypto/golden-campaign-v3-legacy-go.db"
MASTER_KEY = b"symaira-eraseme-golden-master-32"
OFFICIAL_GO_V0121_SHA256 = gate.OFFICIAL_GO_V0121_SHA256
CURRENT_GO_SHA256 = "f1dd5510150995ee99e9b8d21bbec333e27613b7f070f1a34d5cdedd2081023a"
CASES = ("go-refuses-encv3-negative-control", "current-go-initial-read",
         "current-go-write-four", "rust-four-readback", "rust-decrypt-clone", "official-go-read-four",
         "official-go-write-fifth", "official-go-read-five", "rust-verify-five",
         "rust-reencrypt-encv3", "rust-reopen-encv3-five")


def run(current_go, go, rust, output):
    gate.require(os.sys.platform == "darwin" or os.sys.platform.startswith("linux"),
                 "unsupported: switchback confinement is available on macOS and Linux")
    if os.sys.platform.startswith("linux"):
        gate.require(os.getuid() != 0 and os.getgid() != 0,
                     "Linux sandbox runner must start as an unprivileged user")
        gate.require(gate.platform.machine().lower() in ("aarch64", "arm64"),
                     "Linux disposable switchback requires native aarch64")

    output = Path(output)
    gate.require(not output.is_symlink(), "switchback output root must not be a symlink")
    output = output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {"scope": "macos-encrypted-disposable-rollback-bridge" if os.sys.platform == "darwin"
              else "linux-aarch64-encrypted-disposable-rollback-bridge",
              "status": "failed", "platform": {"system": platform.system(),
              "machine": platform.machine()}, "required_cases": list(CASES), "steps": [],
              "production_cutover_verified": False,
              "source_binding": "input hashes identify artifacts; no source-tree/build attestation is checked by this runner",
              "rust_source_binding": "provided integrated release hash; hash alone does not prove source identity"}
    try:
        for name in ("data", "home", "config", "cache", "tmp", "bin", "empty-path"):
            (output / name).mkdir(mode=0o700)
        database = output / "data/symeraseme.db"
        fixture_identity = gate.identity(FIXTURE)
        current_go, go, rust = (Path(path).resolve(strict=True) for path in (current_go, go, rust))
        report["fixture"] = fixture_identity
        report["runner_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        report["artifacts"] = {"current_go": gate.identity(current_go),
                               "official_go_v0.12.1": gate.identity(go), "rust": gate.identity(rust)}
        gate.require(report["artifacts"]["current_go"]["sha256"] == CURRENT_GO_SHA256,
                     "current Go artifact differs from the recorded integrated candidate")
        gate.require(report["artifacts"]["official_go_v0.12.1"]["sha256"] == OFFICIAL_GO_V0121_SHA256,
                     "Go artifact is not the SHA-pinned official v0.12.1 release")
        shutil.copyfile(FIXTURE, database)
        gate.require(database.read_bytes().startswith(b"SYMERASEME_ENCv3\n"),
                     "fixture is not an encrypted V3 database")
        original = output / "rollback-backup/original-encrypted-v3.db"
        original.parent.mkdir(mode=0o700)
        shutil.copyfile(database, original)
        report["original_fixture_envelope"] = gate.identity(original)

        active = output / "bin/symeraseme"
        key_hex = MASTER_KEY.hex()
        env = {"HOME": str(output / "home"), "USERPROFILE": str(output / "home"),
               "XDG_CONFIG_HOME": str(output / "config"), "XDG_DATA_HOME": str(output / "data"),
               "XDG_CACHE_HOME": str(output / "cache"), "TMPDIR": str(output / "tmp"),
               "TEMP": str(output / "tmp"), "TMP": str(output / "tmp"),
               "PATH": str(output / "empty-path"), "LC_ALL": "C", "TZ": "UTC",
               "SYMERASEME_DATA_DIR": str(database.parent), "SYMERASEME_DB_DIR": str(database.parent),
               "SYMERASEME_ENCRYPT_DB": "true", "SYMERASEME_IDENTITY_MASTER_KEY": key_hex}

        def execute(label, artifact, args, command_env=None, expected_exit_code=0):
            command_env = env if command_env is None else command_env
            step = {"id": label, "success": False}
            report["steps"].append(step)
            stage = active.with_suffix(".next")
            shutil.copyfile(artifact, stage)
            stage.chmod(0o700)
            gate.require(gate.identity(stage) == gate.identity(artifact), "staged executable mismatch")
            os.replace(stage, active)
            step["installed"] = gate.identity(active)
            try:
                step["command"] = gate.command(
                    output, label,
                    gate.sandbox_command(output, label, active, args, command_env), command_env,
                    expected_exit_code=expected_exit_code)
            finally:
                record = output / (label + ".json")
                if record.exists():
                    step["command"] = json.loads(record.read_bytes())
            if expected_exit_code == 0:
                step["success"] = True
                return json.loads((output / (label + ".stdout")).read_bytes())
            return None

        # Prove the old release does not open an ENCv3 envelope. Only this
        # disposable copy is presented to it; its bytes must remain unchanged.
        negative_dir = output / "negative"
        negative_dir.mkdir(mode=0o700)
        negative_db = negative_dir / "symeraseme.db"
        shutil.copyfile(original, negative_db)
        negative_identity = gate.identity(negative_db)
        negative_env = dict(env, SYMERASEME_DATA_DIR=str(negative_dir),
                            SYMERASEME_DB_DIR=str(negative_dir), SYMERASEME_ENCRYPT_DB="false")
        execute("go-refuses-encv3-negative-control", go,
                ["requests", "list", "--output", "json"], negative_env, expected_exit_code=1)
        stderr = (output / "go-refuses-encv3-negative-control.stderr").read_bytes()
        gate.require(b"file is not a database" in stderr,
                     "negative control did not show Go's plaintext SQLite rejection")
        gate.require(gate.identity(negative_db) == negative_identity,
                     "negative control altered its ENCv3 input")
        report["steps"][-1]["success"] = True
        report["negative_control"] = {
            "result": "official Go v0.12.1 cannot read the ENCv3 envelope directly",
            "database_unchanged": True, "identity": negative_identity}

        before = execute("current-go-initial-read", current_go, ["requests", "list", "--output", "json"])
        gate.require(before.get("total") == 3 and len(before.get("requests", [])) == 3,
                     "current Go did not read the three fixture requests")
        report["steps"][-1]["success"] = True
        campaign = "rollback-bridge-current-go"
        created = execute("current-go-write-four", current_go,
                          ["plan", "create", "--campaign", campaign, "--max", "1",
                           "--profile", str(output / "home/absent-profile.enc"), "--output", "json"])
        gate.require(created.get("campaign_id") == campaign and created.get("planned") == 1,
                     "current Go did not create the fourth request")
        report["steps"][-1]["success"] = True
        four = execute("rust-four-readback", rust, ["requests", "list", "--output", "json"])
        gate.require(four.get("total") == 4 and len(four.get("requests", [])) == 4,
                     "current Rust did not verify four current-Go requests")
        report["steps"][-1]["success"] = True
        gate.require(database.read_bytes().startswith(b"SYMERASEME_ENCv3\n"),
                     "Rust did not leave an ENCv3 artifact after writing four requests")
        original_identity = gate.identity(database)
        rollback_copy = output / "rollback-backup/rust-four-encrypted-v3.db"
        shutil.copyfile(database, rollback_copy)
        report["post_current_go_four_envelope"] = gate.identity(rollback_copy)

        # Rust's normal plain-store open decrypts an encrypted path in place
        # when encryption is disabled. Apply it only to this clone.
        bridge_dir = output / "bridge"
        bridge_dir.mkdir(mode=0o700)
        bridge_db = bridge_dir / "symeraseme.db"
        shutil.copyfile(rollback_copy, bridge_db)
        plain_env = dict(env, SYMERASEME_DATA_DIR=str(bridge_dir), SYMERASEME_DB_DIR=str(bridge_dir),
                         SYMERASEME_ENCRYPT_DB="false")
        decrypted = execute("rust-decrypt-clone", rust, ["requests", "list", "--output", "json"], plain_env)
        gate.same(decrypted, four, "Rust plaintext clone export differs from encrypted four-request state")
        gate.require(bridge_db.read_bytes().startswith(b"SQLite format 3\x00"),
                     "Rust did not produce a plaintext SQLite clone")
        report["steps"][-1]["success"] = True
        source_state = gate.snapshot(bridge_db)
        gate.require(source_state["user_version"] == 2 and source_state["integrity"] == [("ok",)],
                     "plaintext clone is not an intact schema-v2 database")
        gate.require(len(source_state["tables"]["removal_requests"]["rows"]) == 4,
                     "plaintext clone does not contain four requests")

        with closing(sqlite3.connect(bridge_db)) as clone:
            clone.execute("PRAGMA user_version = 1")
            clone.commit()
        downgraded = gate.snapshot(bridge_db)
        gate.same(gate.without_user_version(downgraded), gate.without_user_version(source_state),
                  "bridge downgrade changed state beyond user_version")
        gate.require(downgraded["user_version"] == 1,
                     "bridge clone did not reach schema-v1 compatibility marker")
        report["rollback_bridge"] = {
            "scope": "isolated plaintext clone only", "official_go_version": "v0.12.1",
            "official_go_sha256": OFFICIAL_GO_V0121_SHA256,
            "encrypted_source": str(rollback_copy), "encrypted_source_sha256": original_identity["sha256"],
            "plaintext_clone": str(bridge_db), "state_before_downgrade": source_state,
            "state_after_downgrade": downgraded, "downgrade_changed_only_user_version": True}

        original_rows = execute("official-go-read-four", go,
                                ["requests", "list", "--output", "json"], plain_env)
        gate.require(original_rows.get("total") == 4 and len(original_rows.get("requests", [])) == 4,
                     "official Go v0.12.1 did not read all four bridged requests")
        gate.same(sorted(map(gate.canonical, original_rows["requests"])),
                  sorted(map(gate.canonical, four["requests"])),
                  "official Go changed or missed a Rust-era request")
        report["steps"][-1]["success"] = True
        unique_campaign = "rollback-bridge-" + os.urandom(8).hex()
        bridge_create = execute("official-go-write-fifth", go,
                                ["plan", "create", "--campaign", unique_campaign, "--max", "1",
                                 "--profile", str(output / "home/bridge-absent-profile.enc"),
                                 "--output", "json"], plain_env)
        gate.require(bridge_create.get("campaign_id") == unique_campaign and bridge_create.get("planned") == 1,
                     "official Go v0.12.1 did not create one fifth request")
        report["steps"][-1]["success"] = True
        five = execute("official-go-read-five", go,
                       ["requests", "list", "--output", "json"], plain_env)
        gate.require(five.get("total") == 5 and len(five.get("requests", [])) == 5,
                     "official Go v0.12.1 could not read its fifth request")
        old = set(map(gate.canonical, original_rows["requests"]))
        gate.require(sum(gate.canonical(row) not in old for row in five["requests"]) == 1
                     and all(sum(gate.canonical(row) == gate.canonical(candidate)
                                 for candidate in five["requests"]) == 1
                             for row in original_rows["requests"]),
                     "Go bridge write did not preserve four old requests and add exactly one")
        report["steps"][-1]["success"] = True

        rust_five = execute("rust-verify-five", rust,
                            ["requests", "list", "--output", "json"], plain_env)
        gate.same(rust_five, five, "current Rust cannot verify all five plaintext bridge requests")
        rust_state = gate.snapshot(bridge_db)
        gate.require(rust_state["user_version"] == 2 and rust_state["integrity"] == [("ok",)],
                     "current Rust did not reopen the bridge clone as intact schema v2")
        report["steps"][-1]["success"] = True
        report["rollback_bridge"]["rust_after_five"] = rust_state

        # Re-encrypt the same five-request clone using the production Rust path.
        encrypted_dir = output / "re-encrypted"
        encrypted_dir.mkdir(mode=0o700)
        encrypted_db = encrypted_dir / "symeraseme.db"
        shutil.copyfile(bridge_db, encrypted_db)
        encrypted_env = dict(env, SYMERASEME_DATA_DIR=str(encrypted_dir),
                             SYMERASEME_DB_DIR=str(encrypted_dir), SYMERASEME_ENCRYPT_DB="true")
        encrypted_read = execute("rust-reencrypt-encv3", rust,
                                 ["requests", "list", "--output", "json"], encrypted_env)
        gate.same(encrypted_read, five, "Rust encrypted reopen changed the five-request state")
        encrypted_identity = gate.identity(encrypted_db)
        gate.require(encrypted_db.read_bytes().startswith(b"SYMERASEME_ENCv3\n"),
                     "Rust did not re-encrypt the bridge state as ENCv3")
        report["steps"][-1]["success"] = True
        reopened = execute("rust-reopen-encv3-five", rust,
                           ["requests", "list", "--output", "json"], encrypted_env)
        gate.same(reopened, five, "Rust could not reopen its new ENCv3 five-request artifact")
        report["steps"][-1]["success"] = True
        report["new_encrypted_artifact"] = {**encrypted_identity, "envelope": "SYMERASEME_ENCv3",
                                            "requests": 5, "path": str(encrypted_db)}
        gate.require(gate.identity(database) == original_identity,
                     "rollback bridge changed the original ENCv3 database bytes")
        gate.require(gate.identity(FIXTURE) == fixture_identity,
                     "source fixture changed during the rehearsal")
        gate.require([step["id"] for step in report["steps"]] == report["required_cases"]
                     and all(step["success"] is True for step in report["steps"]),
                     "incomplete case inventory")
        report["original_envelope_unchanged"] = True
        report["status"] = "rollback-bridge-passed"
    finally:
        gate.save(output / "report.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True,
                        help="official SHA-pinned v0.12.1 release executable")
    parser.add_argument("--current-go", type=Path, required=True,
                        help="recorded integrated current Go executable that writes request four")
    parser.add_argument("--rust", type=Path, required=True,
                        help="existing current Rust executable; this script does not build it")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    result = run(args.current_go, args.go, args.rust, args.output_dir)
    print(json.dumps({"status": result["status"], "scope": result["scope"],
                      "executed_cases": len(result["steps"]),
                      "original_envelope_unchanged": result.get("original_envelope_unchanged", False)}))


if __name__ == "__main__":
    main()
