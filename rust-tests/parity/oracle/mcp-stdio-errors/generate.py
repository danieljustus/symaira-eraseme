#!/usr/bin/env python3
"""Record Go's MCP stdio syntax errors from the pinned source revision."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[4]
cases = {
    "truncated": b'{"jsonrpc":"2.0",',
    "junk-after-response": b'{"jsonrpc":"2.0","id":1,"method":"initialize"} @@',
    "malformed-literal": b"nope\n",
    "invalid-string-escape": b'"bad\\x"',
    "array-trailing-comma": b"[1,]",
    "object-trailing-comma": b'{"x":1,}',
    "literal-then-junk": b"truex",
    "adjacent-valid-scalars": b"01",
}
go = Path(sys.argv[1]).resolve()
assert "go1.26.6" in subprocess.check_output([str(go), "version"], text=True)
subprocess.run(
    ["git", "diff", "--exit-code", "4e582f28", "--", "internal/mcp/server.go", "cmd/symeraseme/main.go"],
    cwd=root,
    check=True,
    stdout=subprocess.DEVNULL,
)
recorded = []
with tempfile.TemporaryDirectory(prefix="symeraseme-mcp015-") as directory:
    binary = Path(directory) / "symeraseme-go"
    subprocess.run([str(go), "build", "-o", str(binary), "./cmd/symeraseme"], cwd=root, check=True)
    for name, data in cases.items():
        result = subprocess.run(
            [str(binary), "mcp", "--stdio"],
            input=data,
            capture_output=True,
            check=False,
            timeout=15,
        )
        recorded.append(
            {
                "name": name,
                "input": data.decode(),
                "exit_code": result.returncode,
                "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode(),
            }
        )
target = root / "rust-tests/parity/cases/mcp/stdio-errors.json"
target.write_text(json.dumps({"source_revision": "4e582f28", "cases": recorded}, indent=2) + "\n")
