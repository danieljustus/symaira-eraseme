# Task 2.1 specification review

- Issue: #804
- Task: 2.1 — Port version and build metadata
- Reviewed SHA: `4f1f467e73a5b094d322c11c60f90640d5c617c2`
- Reviewer lane: specification
- Verdict: **PASS**

## Exact oracle evidence

With the Rust package version set to `0.13.0`, the Rust shadow executable
matches the isolated Go oracle byte-for-byte:

- root `--version`: `symeraseme version 0.13.0\n`
- `version`: `symeraseme 0.13.0\n`
- `version --json`: `{"tool":"symeraseme","version":"0.13.0","schema_version":1}\n`
- extra positional arguments: exit code `1`, no internal `symeraseme-rust`
  name leakage

The version module uses deterministic Cargo build metadata, preserves JSON
field order/newlines, and safely escapes version text. Scope is limited to the
version slice; phase-1 harness, Go code and frozen fixtures remain unchanged.

## Verification

- Rust 1.98 format/check/Clippy/workspace tests — exit 0
- focused version integration tests — exit 0
- direct Go/Rust byte comparisons — pass
- Go regression gate — exit 0
- added-line secret/path and scope checks — pass
