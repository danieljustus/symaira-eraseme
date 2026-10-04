#!/usr/bin/env python3
"""Record Go CLI process bytes for malformed and MCP stdio boundary inputs."""

import argparse
import base64
import hashlib
import json
import random
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path


class OracleTemporaryDirectory(tempfile.TemporaryDirectory):
    """Retry only a transient Windows sharing violation on this owned root."""

    def cleanup(self, *, timeout=5.0):
        deadline = time.monotonic() + timeout
        while True:
            try:
                super().cleanup()
                return
            except OSError as error:
                remaining = deadline - time.monotonic()
                if (sys.platform != "win32" or getattr(error, "winerror", None) != 32
                        or remaining <= 0):
                    raise
                time.sleep(min(0.05, remaining))

ROOT = Path(__file__).resolve().parents[4]
SOURCE_REVISION = "0bc6b051890340822448fa3846c96f70022b79ad"
INITIALIZE = ROOT / "tests/fixtures/mcp-contract/initialize_cases.json"
SOURCE_PATHS = ["cmd/symeraseme/main.go", "internal/mcp/server.go"]
MUTATION_SEED = 0x4D43503135
MUTATION_COUNT = 128
MUTATION_BASE = b'{"jsonrpc":"2.0","id":1,"method":"initialize"}'


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
    mutations = []
    rng = random.Random(MUTATION_SEED)
    for index in range(MUTATION_COUNT):
        data = bytearray(MUTATION_BASE)
        operation = index % 3
        if operation == 0:
            offset = rng.randrange(len(data))
            data[offset] = rng.choice(b'"\\,]}x:')
        elif operation == 1:
            offset = rng.randrange(len(data))
            del data[offset]
        else:
            offset = rng.randrange(len(data) + 1)
            data[offset:offset] = bytes([rng.choice(b'"\\,]}x:')])
        mutations.append(
            {
                "name": f"seeded-byte-mutation-{index:02d}",
                "input_spec": {"base64": base64.b64encode(data).decode()},
            }
        )
    # Source-derived diagnostics beyond the initialize-shaped mutation seed.
    syntax = [
        ("array-string-separator", b'["a" 1]'),
        ("array-number-separator", b'[1n]'),
        ("key-hex-escape", b'{"\\u00zz":1}'),
        ("value-hex-escape", b'{"a":"\\u00zz"}'),
        ("multiline-key-separator", b'{\n"a"\n1}'),
        ("multiline-array-separator", b'[\n1\n2]'),
        ("multiline-value-separator", b'{\n"a":1\n2}'),
        ("numeric-minus", b'[-x]'),
        ("numeric-fraction", b'[1.x]'),
        ("numeric-exponent", b'[1ex]'),
        ("numeric-signed-exponent", b'[1e+x]'),
        ("nested-array-separator", b'{"a":[[1 2]]}'),
        ("earliest-error-before-escape", b'[x,"\\q"]'),
        ("earliest-error-before-literal", b'{"a" 1,"b":truX}'),
        ("truncated-escape", b'"a\\'),
        ("truncated-hex-escape", b'"\\u00'),
        ("truncated-literal", b'[tru'),
        ("truncated-fraction", b'[1.'),
        ("truncated-exponent", b'[1e+'),
        ("truncated-string", b'"abc'),
    ]
    # Every possible offending byte covers Go quoteChar's Latin-1/control
    # spelling and every legal/illegal escape, not Rust-authored expectations.
    syntax += [(f"key-byte-{byte:02x}", b'{' + bytes([byte]) + b'}')
               for byte in range(256)]
    syntax += [(f"escape-byte-{byte:02x}", b'"\\' + bytes([byte]) + b'"')
               for byte in range(256)]
    diagnostics = [
        {"name": "syntax-" + name,
         "input_spec": {"base64": base64.b64encode(data).decode()}}
        for name, data in syntax
    ]
    ping = [
        {"name": "ping-" + name,
         "input_spec": {"base64": base64.b64encode(data).decode()}}
        for name, data in [
            ("request", b'{"jsonrpc":"2.0","id":1,"method":"ping"}\n'),
            ("null-params", b'{"jsonrpc":"2.0","id":1.0,"method":"ping","params":null}\n'),
            ("null-id", b'{"jsonrpc":"2.0","id":null,"method":"ping","params":{}}\n'),
            ("escaped-id", b'{"jsonrpc":"2.0","id":"<&>","method":"ping"}\n'),
            ("invalid-params", b'{"jsonrpc":"2.0","id":1,"method":"ping","params":[]}\n'),
            ("invalid-id", b'{"jsonrpc":"2.0","id":true,"method":"ping"}\n'),
            ("notification", b'{"jsonrpc":"2.0","method":"ping"}\n'),
            ("invalid-params-notification", b'{"jsonrpc":"2.0","method":"ping","params":[]}\n'),
        ]
    ]
    return malformed + [
        {"name": "size-below-8k", "input_spec": {"kind": "padding_request", "size": 8192 - 1}},
        {"name": "size-above-8k", "input_spec": {"kind": "padding_request", "size": 8192 + 1}},
        {"name": "nesting-at-go-limit", "input_spec": {"kind": "nested_request", "array_depth": 9999}},
        {"name": "nesting-over-go-limit", "input_spec": {"kind": "nested_request", "array_depth": 10000}},
    ] + mutations + diagnostics + ping


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


def validate_go_version(version):
    if not isinstance(version, str) or not re.fullmatch(
        r"go version go1\.26\.6 (darwin|linux|windows)/(amd64|arm64)", version
    ):
        raise ValueError("expected Go 1.26.6 with a supported native host suffix")


def capture(go):
    version = subprocess.check_output([str(go), "version"], text=True).strip()
    validate_go_version(version)
    subprocess.run(
        ["git", "diff", "--exit-code", SOURCE_REVISION, "--", *SOURCE_PATHS],
        cwd=ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
    )
    recorded = []
    with OracleTemporaryDirectory(prefix="symeraseme-mcp008-") as directory:
        binary = Path(directory) / "symeraseme-go.exe"
        subprocess.run([str(go), "build", "-o", str(binary), "./cmd/symeraseme"], cwd=ROOT, check=True)
        for case in case_specs():
            with OracleTemporaryDirectory(dir=directory, prefix="case-") as scenario:
                result = subprocess.run(
                    [str(binary), "mcp", "--stdio"],
                    cwd=scenario,
                    input=materialize(case["input_spec"]),
                    capture_output=True,
                    check=False,
                    timeout=30,
                    env={
                        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                        "HOME": scenario, "USERPROFILE": scenario,
                        "TMPDIR": scenario, "TMP": scenario, "TEMP": scenario,
                        **{f"XDG_{name}_HOME": str(Path(scenario) / name.lower())
                           for name in ("CONFIG", "DATA", "STATE", "CACHE")},
                    },
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
    return {
        "source_revision": SOURCE_REVISION,
        "mutation_seed": MUTATION_SEED,
        "mutation_count": MUTATION_COUNT,
        "go_version": version,
        "source_files": [
            {"path": path, "sha256": digest((ROOT / path).read_bytes())} for path in SOURCE_PATHS
        ],
        "initialize_fixture_sha256": digest(INITIALIZE.read_bytes()),
        "generator_sha256": digest(Path(__file__).read_bytes()),
        "cases": recorded,
    }


def check(target, observed):
    expected = json.loads(target.read_bytes())
    # Only the compiler's host suffix may differ across native CI targets.
    validate_go_version(expected["go_version"])
    validate_go_version(observed["go_version"])
    expected["go_version"] = observed["go_version"]
    if json.dumps(expected, sort_keys=True) != json.dumps(observed, sort_keys=True):
        raise ValueError("MCP stdio oracle drift; regenerate explicitly after review")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("go", type=Path, help="Go 1.26.6 executable")
    parser.add_argument("--check", action="store_true", help="compare without writing")
    parser.add_argument("--output", type=Path,
                        default=Path(__file__).with_name("cases.json"))
    args = parser.parse_args()
    observed = capture(args.go.resolve())
    if args.check:
        check(args.output, observed)
        print(f"MCP stdio oracle: {len(observed['cases'])} cases checked, no writes")
    else:
        args.output.write_text(json.dumps(observed, indent=2) + "\n")


if __name__ == "__main__":
    main()
