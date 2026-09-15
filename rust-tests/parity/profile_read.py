#!/usr/bin/env python3
"""Capture/replay profiles through immutable Go, then execute the Rust consumer.

Ciphertext is randomized by the real production encryptor. Check mode replays
retained bytes through Go LoadProfile; it never regenerates expectations in CI.
"""
import argparse
import hashlib
import io
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ORACLE = "bf53346eec234929bedf0314b99e3da85dbb991b"
HELPER = ROOT / "rust-tests/parity/oracle/profile_read/main.go"
FIXTURE = ROOT / "rust-tests/parity/oracle/profile_read/cases.json"


def run(args, *, cwd=ROOT, env=None, data=None, timeout=300):
    result = subprocess.run(args, cwd=cwd, env=env, input=data,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=timeout, check=False)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed ({result.returncode}): "
                           + result.stderr.decode(errors="replace")
                           + result.stdout.decode(errors="replace"))
    return result.stdout


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def source_manifest():
    names = run(["git", "ls-tree", "-r", "--name-only", ORACLE,
                 "go.mod", "go.sum", "internal/identity", "internal/eventstore"]).decode().splitlines()
    return {name: digest(run(["git", "show", f"{ORACLE}:{name}"])) for name in names}


def verify_capture(capture, manifest):
    assert capture["oracle"] == ORACLE
    assert capture["sources"] == manifest, "oracle source manifest mismatch"
    assert capture["helper_sha256"] == digest(HELPER.read_bytes()), "helper drift"
    assert capture["validator_sha256"] == digest(Path(__file__).read_bytes()), "validator drift"
    names = [case["name"] for case in capture["cases"]]
    assert len(names) == len(set(names)) == 46, "case inventory mismatch"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--generate", action="store_true")
    parser.add_argument("--mutation-check", action="store_true")
    parser.add_argument("--rust", action="store_true")
    args = parser.parse_args()
    actual = run(["git", "rev-parse", f"{ORACLE}^{{commit}}"]).decode().strip()
    assert actual == ORACLE
    print(json.dumps({"candidate": run(["git", "rev-parse", "HEAD"]).decode().strip(),
                      "worktree": str(ROOT), "os": platform.system(), "arch": platform.machine(),
                      "dirty_paths": run(["git", "status", "--porcelain"]).decode().splitlines(),
                      "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes())}), flush=True)
    manifest = source_manifest()
    with tempfile.TemporaryDirectory(prefix="eraseme-profile-oracle-") as temporary:
        temp = Path(temporary)
        archive = run(["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf",
                       "archive", ORACLE, "go.mod", "go.sum", "internal/identity", "internal/eventstore"])
        with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
            tree.extractall(temp, filter="data")
        for name, expected in manifest.items():
            assert digest((temp / name).read_bytes()) == expected
        adapter = temp / "oracle/main.go"
        adapter.parent.mkdir()
        shutil.copyfile(HELPER, adapter)
        executable = temp / ("oracle.exe" if os.name == "nt" else "oracle-bin")
        build_env = {k: v for k, v in os.environ.items() if not k.startswith(("SYMERASEME_", "SYMVAULT_"))}
        build_env["GOTOOLCHAIN"] = "go1.26.6"
        build_env["GOWORK"] = "off"
        run(["go", "build", "-o", str(executable), "./oracle"], cwd=temp, env=build_env)
        runtime_env = {k: v for k, v in build_env.items() if k in ("PATH", "SystemRoot", "SYSTEMROOT", "WINDIR")}
        runtime_env.update({"HOME": str(temp), "USERPROFILE": str(temp), "TMPDIR": str(temp),
                            "TMP": str(temp), "TEMP": str(temp), "LANG": "C", "TZ": "UTC"})
        if args.generate:
            cases = json.loads(run([str(executable)], env=runtime_env))
            capture = {"oracle": ORACLE, "sources": manifest,
                       "helper_sha256": digest(HELPER.read_bytes()),
                       "validator_sha256": digest(Path(__file__).read_bytes()), "cases": cases}
            verify_capture(capture, manifest)
            FIXTURE.write_text(json.dumps(capture, indent=2, ensure_ascii=True) + "\n")
        capture = json.loads(FIXTURE.read_bytes())
        verify_capture(capture, manifest)
        observed = json.loads(run([str(executable), "replay"], env=runtime_env,
                                  data=json.dumps(capture["cases"]).encode()))
        # Native OS error prose is not portable. Retain it in the capture, but
        # compare the typed stat/read outcome and all other observations.
        expected_cases = json.loads(json.dumps(capture["cases"]))
        for actual_case, expected_case in zip(observed, expected_cases, strict=True):
            if expected_case["expected"]["class"] in ("stat", "read"):
                actual_case["expected"]["error"] = "OS diagnostic (classified)"
                expected_case["expected"]["error"] = "OS diagnostic (classified)"
        assert observed == expected_cases, "Go replay differs from capture"
        print(f"Go production replay PASS: {len(observed)} cases; {ORACLE}", flush=True)
    if args.rust or args.mutation_check:
        metadata = json.loads(run(["cargo", "metadata", "--manifest-path", str(ROOT / "Cargo.toml"),
                                   "--no-deps", "--format-version", "1"]))
        assert Path(metadata["workspace_root"]).resolve() == ROOT
        core = next(p for p in metadata["packages"] if p["name"] == "symeraseme-core")
        assert Path(core["manifest_path"]).resolve() == ROOT / "crates/symeraseme-core/Cargo.toml"
        assert all(Path(t["src_path"]).resolve().is_relative_to(ROOT) for t in core["targets"])
        command = ["cargo", "test", "--locked", "--manifest-path", str(ROOT / "Cargo.toml"),
                   "-p", "symeraseme-core", "--test", "identity_profile", "--", "--nocapture"]
        result = run(command, timeout=600).decode()
        assert "0 failed" in result and "profile_corpus_matches_go" in result, result
        print(result, end="")
        if args.mutation_check:
            original = FIXTURE.read_bytes()
            try:
                mutated = json.loads(original)
                mutated["cases"][0]["expected"]["profile"]["full_name"] = "MUTATION MUST FAIL"
                FIXTURE.write_text(json.dumps(mutated))
                rejected = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=600)
                assert rejected.returncode != 0, "Rust comparator accepted changed Go expectation"
                assert b"profile_corpus_matches_go" in rejected.stdout
                assert b"MUTATION MUST FAIL" in rejected.stdout + rejected.stderr
                print("Mutation killed by real Rust comparator", flush=True)
            finally:
                FIXTURE.write_bytes(original)
            print(run(command, timeout=600).decode(), end="")
            print("Restoration PASS", flush=True)


if __name__ == "__main__":
    main()
