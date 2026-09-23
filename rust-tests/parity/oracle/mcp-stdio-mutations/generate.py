#!/usr/bin/env python3
"""Record Go CLI process bytes for malformed and MCP stdio boundary inputs."""

import base64
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
INITIALIZE = ROOT / "tests/fixtures/mcp-contract/initialize_cases.json"
SOURCE_PATHS = ["cmd/symeraseme/main.go", "internal/mcp/server.go"]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def case_specs():
    fixture = json.loads(INITIALIZE.read_text())
    malformed = []
    for case in fixture["cases"]:
        if not case.get("parse_error"):
            continue
        request = (
            base64.b64decode(case["request_b64"])
            if case.get("request_b64")
            else case["request"].encode()
        )
        malformed.append(
            {"name": case["name"], "input_spec": {"base64": base64.b64encode(request).decode()}}
        )
    assert len(malformed) == 6, "the source initialize fixture no longer has six parse errors"
    return malformed + [
        {"name": "size-below-8k", "input_spec": {"kind": "padding_request", "size": 8192 - 1}},
        {"name": "size-above-8k", "input_spec": {"kind": "padding_request", "size": 8192 + 1}},
        {"name": "nesting-at-go-limit", "input_spec": {"kind": "nested_request", "array_depth": 9999}},
        {"name": "nesting-over-go-limit", "input_spec": {"kind": "nested_request", "array_depth": 10000}},
    ]


def materialize(spec):
    if "base64" in spec:
        return base64.b64decode(spec["base64"])
    if spec["kind"] == "padding_request":
        prefix, suffix = b'{"padding":"', b'"}'
        assert spec["size"] >= len(prefix) + len(suffix)
        return prefix + b"x" * (spec["size"] - len(prefix) - len(suffix)) + suffix
    if spec["kind"] == "nested_request":
        depth = spec["array_depth"]
        return (
            b'{"jsonrpc":"2.0","id":1,"method":"initialize","ignored":'
            + b"[" * depth
            + b"0"
            + b"]" * depth
            + b"}"
        )
    raise AssertionError(f"unknown input spec: {spec}")


def main():
    go = Path(sys.argv[1]).resolve()
    target = ROOT / "rust-tests/parity/oracle/mcp-stdio-mutations/cases.json"
    recorded = []
    with tempfile.TemporaryDirectory(prefix="symeraseme-mcp008-") as directory:
        binary = Path(directory) / "symeraseme-go"
        subprocess.run([str(go), "build", "-o", str(binary), "./cmd/symeraseme"], cwd=ROOT, check=True)
        for case in case_specs():
            result = subprocess.run(
                [str(binary), "mcp", "--stdio"],
                input=materialize(case["input_spec"]),
                capture_output=True,
                check=False,
                timeout=30,
                env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": directory, "TMPDIR": directory},
            )
            recorded.append(
                {
                    "name": case["name"],
                    "input_spec": case["input_spec"],
                    "exit_code": result.returncode,
                    "stdout_base64": base64.b64encode(result.stdout).decode(),
                    "stderr_base64": base64.b64encode(result.stderr).decode(),
                }
            )
    fixture = {
        "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "go_version": subprocess.check_output([str(go), "version"], text=True).strip(),
        "source_files": [
            {"path": path, "sha256": digest((ROOT / path).read_bytes())} for path in SOURCE_PATHS
        ],
        "initialize_fixture_sha256": digest(INITIALIZE.read_bytes()),
        "generator_sha256": digest(Path(__file__).read_bytes()),
        "cases": recorded,
    }
    target.write_text(json.dumps(fixture, indent=2) + "\n")


if __name__ == "__main__":
    main()
