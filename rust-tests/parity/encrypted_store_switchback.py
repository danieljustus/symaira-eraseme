#!/usr/bin/env python3
"""Bounded macOS encrypted-store Go -> Rust -> Go CLI interop check.

This is local evidence only. It does not authorize a production cutover.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil

import plain_store_switchback as gate


FIXTURE = gate.REPO / "tests/fixtures/event-store/crypto/golden-campaign-v3-legacy-go.db"
MASTER_KEY = b"symaira-eraseme-golden-master-32"
CASES = ("go-existing-state", "rust-existing-state", "rust-write",
         "rust-readback", "go-after-rust")


def run(go, rust, output):
    gate.require(os.sys.platform == "darwin", "unsupported: this gate requires macOS sandbox-exec")
    output = output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {"scope": "macos-encrypted-store-runtime-only", "status": "failed",
              "platform": {"system": platform.system(), "machine": platform.machine()},
              "required_cases": list(CASES), "steps": [], "database_restore_performed": False,
              "production_cutover_verified": False,
              "source_binding": "binary hashes identify inputs; hashes alone do not prove source identity"}
    try:
        for name in ("data", "home", "config", "cache", "tmp", "bin", "empty-path"):
            (output / name).mkdir(mode=0o700)
        database = output / "data/symeraseme.db"
        fixture_identity = gate.identity(FIXTURE)
        report["fixture"] = fixture_identity
        report["runner_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        go, rust = (path.resolve(strict=True) for path in (go, rust))
        report["artifacts"] = {"go": gate.identity(go), "rust": gate.identity(rust)}
        gate.require(report["artifacts"]["go"]["sha256"] != report["artifacts"]["rust"]["sha256"],
                     "Go and Rust artifacts must differ")
        shutil.copyfile(FIXTURE, database)
        active = output / "bin/symeraseme"
        key_hex = MASTER_KEY.hex()
        env = {"HOME": str(output / "home"), "USERPROFILE": str(output / "home"),
               "XDG_CONFIG_HOME": str(output / "config"), "XDG_DATA_HOME": str(output / "data"),
               "XDG_CACHE_HOME": str(output / "cache"), "TMPDIR": str(output / "tmp"),
               "TEMP": str(output / "tmp"), "TMP": str(output / "tmp"),
               "PATH": str(output / "empty-path"), "LC_ALL": "C", "TZ": "UTC",
               "SYMERASEME_DATA_DIR": str(output / "data"),
               "SYMERASEME_DB_DIR": str(output / "data"),
               "SYMERASEME_ENCRYPT_DB": "true",
               "SYMERASEME_IDENTITY_MASTER_KEY": key_hex}
        policy = gate.sandbox(output, active.resolve())
        gate.save(output / "sandbox-policy.json", policy)

        def execute(label, artifact, args):
            step = {"id": label, "success": False}
            report["steps"].append(step)
            stage = active.with_suffix(".next")
            shutil.copyfile(artifact, stage)
            stage.chmod(0o700)
            gate.require(gate.identity(stage) == gate.identity(artifact), "staged executable mismatch")
            os.replace(stage, active)
            step["installed"] = gate.identity(active)
            try:
                gate.command(output, label,
                             ["/usr/bin/sandbox-exec", "-p", policy, str(active), *args], env)
            finally:
                record = output / (label + ".json")
                if record.exists():
                    step["command"] = json.loads(record.read_bytes())
            gate.require(not (output / (label + ".stderr")).read_bytes(), label + ": unexpected stderr")
            step["success"] = True
            return json.loads((output / (label + ".stdout")).read_bytes())

        gate.require(database.read_bytes().startswith(b"SYMERASEME_ENCv3\n"),
                     "fixture is not an encrypted V3 database")
        before = execute("go-existing-state", go,
                         ["requests", "list", "--output", "json"])
        gate.require(type(before["total"]) is int and before["total"] == 3
                     and len(before["requests"]) == 3, "Go did not read all existing requests")
        rust_before = execute("rust-existing-state", rust,
                              ["requests", "list", "--output", "json"])
        gate.same(rust_before, before, "Rust read differs from existing Go encrypted state")
        created = execute("rust-write", rust,
                          ["plan", "create", "--campaign", "post-rust-encrypted", "--max", "1",
                           "--profile", str(output / "home/absent-profile.enc"), "--output", "json"])
        gate.require(created["campaign_id"] == "post-rust-encrypted"
                     and type(created["planned"]) is int and created["planned"] == 1,
                     "Rust did not create exactly one request")
        rust_after = execute("rust-readback", rust,
                             ["requests", "list", "--output", "json"])
        gate.require(rust_after["total"] == 4 and len(rust_after["requests"]) == 4,
                     "Rust write did not persist alongside existing requests")
        go_after = execute("go-after-rust", go,
                           ["requests", "list", "--output", "json"])
        gate.same(go_after, rust_after, "Go cannot read the encrypted state after Rust write")
        gate.require(database.read_bytes().startswith(b"SYMERASEME_ENCv3\n"),
                     "database lost its encrypted envelope")
        gate.require(gate.identity(FIXTURE) == fixture_identity, "source fixture changed")
        gate.require([step["id"] for step in report["steps"]] == list(CASES),
                     "incomplete case inventory")
        report["status"] = "passed"
    finally:
        gate.save(output / "report.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    result = run(args.go, args.rust, args.output_dir)
    print(json.dumps({"status": result["status"], "scope": result["scope"],
                      "executed_cases": len(result["steps"])}))


if __name__ == "__main__":
    main()
