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
import resource
import shutil
import signal
import subprocess

import plain_store_switchback as gate


FIXTURE = gate.REPO / "tests/fixtures/event-store/crypto/golden-campaign-v3-legacy-go.db"
MASTER_KEY = b"symaira-eraseme-golden-master-32"
CASES = ("go-existing-state", "rust-existing-state", "rust-write",
         "rust-mcp-read", "rust-readback", "go-after-rust")


def mcp_command(root, label, argv, env, stdin_bytes, timeout=30):
    """Run one bounded stdio MCP request while retaining protocol evidence."""
    def bounds():
        maximum = 4 * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_FSIZE, (maximum, maximum))

    record = {"argv": list(map(str, argv)), "cwd": str(root), "exit_code": None,
              "timed_out": False, "success": False,
              "stdin": {"size": len(stdin_bytes),
                        "sha256": hashlib.sha256(stdin_bytes).hexdigest()}}
    try:
        with (root / (label + ".stdout")).open("xb") as out, \
                (root / (label + ".stderr")).open("xb") as err:
            child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.PIPE,
                                     stdout=out, stderr=err, start_new_session=True,
                                     preexec_fn=bounds)
            try:
                child.stdin.write(stdin_bytes)
                child.stdin.close()
                child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                record["timed_out"] = True
            finally:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                record["exit_code"] = child.wait(timeout=5)
        record["success"] = record["exit_code"] == 0 and not record["timed_out"]
    finally:
        for stream in ("stdout", "stderr"):
            path = root / (label + "." + stream)
            if path.exists():
                record[stream] = {"path": path.name, **gate.identity(path)}
        gate.save(root / (label + ".json"), record)
    gate.require(record["success"], label + ": MCP process failed; retained raw evidence")
    return record


def run(go, rust, output):
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
    scope = ("linux-aarch64-encrypted-store-disposable-runtime-only"
             if os.sys.platform.startswith("linux") else "macos-encrypted-store-runtime-only")
    report = {"scope": scope, "status": "failed",
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
        if os.sys.platform.startswith("linux"):
            report["sandbox_helper"] = gate.identity(gate.LINUX_SANDBOX)
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
        if os.sys.platform.startswith("linux"):
            python = Path(os.sys.executable).resolve()
            host_write_probe = guest_write_probe = None
            try:
                host_write_probe = gate.create_write_probe(output, "encrypted", host_share=True)
                guest_write_probe = gate.create_write_probe(output, "encrypted", host_share=False)
                gate.command(
                    output, "sandbox-negative",
                    gate.sandbox_command(
                        output, "sandbox-negative", python,
                        ["-I", "-S", "-c", gate.PROBE,
                         str(gate.REPO / "Cargo.toml"), str(Path.home().resolve()),
                         str(gate.REPO / "Cargo.toml"), host_write_probe["path"],
                         guest_write_probe["path"]], env),
                    env)
                controls = json.loads((output / "sandbox-negative.stdout").read_bytes())
                gate.require(set(controls) == {
                    "read_denied_0", "home_directory_read_denied", "network_denied",
                    "udp_network_denied", "outside_write_denied", "host_share_write_denied",
                    "guest_local_write_denied", "child_exec_denied"}
                    and all(value is True for value in controls.values()),
                    "Linux sandbox control failed")
            finally:
                report["outside_write_probes"] = {}
                if host_write_probe is not None:
                    report["outside_write_probes"]["host_share"] = {
                        **host_write_probe, **gate.remove_write_probe(host_write_probe)}
                if guest_write_probe is not None:
                    report["outside_write_probes"]["guest_local"] = {
                        **guest_write_probe, **gate.remove_write_probe(guest_write_probe)}
            audit = json.loads((output / ".sandbox/sandbox-negative.audit.json").read_bytes())
            required = ("mnt", "net", "pid")
            gate.require(audit["status"] == "running" and audit["landlock_abi"] >= 4
                         and audit["no_new_privs"] is True
                         and audit["uid"] == os.getuid() and audit["gid"] == os.getgid()
                         and all(audit["caller_namespace_ids"][name] !=
                                 audit["sandbox_namespace_ids"][name] for name in required)
                         and audit["read_only_virtiofs_mounts"],
                         "Linux namespace or filesystem boundary was not enforced")
            report["sandbox_controls"] = controls
            report["sandbox_isolation"] = audit

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
                             gate.sandbox_command(output, label, active, args, env), env)
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
        # Exercise the same encrypted store through the Rust MCP boundary. This
        # read also verifies the handler explicitly closes/re-encrypts its store.
        step = {"id": "rust-mcp-read", "success": False}
        report["steps"].append(step)
        mcp_request = (b'{"jsonrpc":"2.0","id":"encrypted-switchback",'
                       b'"method":"tools/call","params":{"name":"manual_tasks_list",'
                       b'"arguments":{}}}\n')
        try:
            step["command"] = mcp_command(
                output, "rust-mcp-read",
                gate.sandbox_command(output, "rust-mcp-read", active, ["mcp", "--stdio"], env),
                env, mcp_request)
        finally:
            record = output / "rust-mcp-read.json"
            if record.exists():
                step["command"] = json.loads(record.read_bytes())
        gate.require(not (output / "rust-mcp-read.stderr").read_bytes(),
                     "rust-mcp-read: unexpected stderr")
        mcp_response = json.loads((output / "rust-mcp-read.stdout").read_bytes())
        gate.require(mcp_response.get("id") == "encrypted-switchback"
                     and "error" not in mcp_response,
                     "Rust MCP did not return a successful tool call")
        contents = mcp_response.get("result", {}).get("content", [])
        gate.require(len(contents) == 1 and contents[0].get("type") == "text",
                     "Rust MCP returned an unexpected tool result")
        mcp_result = json.loads(contents[0]["text"])
        gate.require(mcp_result.get("success") is True
                     and isinstance(mcp_result.get("tasks"), list),
                     "Rust MCP failed to read manual tasks from encrypted state")
        step["success"] = True
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
