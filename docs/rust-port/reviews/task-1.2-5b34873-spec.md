# Task 1.2 specification review

- Issue: #803
- Task: 1.2 — Add the neutral differential harness
- Reviewed SHA: `5b34873214b21066f5c6b0810a91f8a481ac1042`
- Previous stage SHA: `ba1a7bcaaecfb00e37fcf05dfbdf86cfb92dd9e7`
- Reviewer lane: specification
- Verdict: **PASS**

## Commands and results

- `cargo +1.98.0 fmt --all --check` — exit 0
- `cargo +1.98.0 check --workspace --all-targets` — exit 0
- `cargo +1.98.0 clippy --workspace --all-targets -- -D warnings` — exit 0
- `cargo +1.98.0 test --workspace --all-targets` — exit 0
- `cargo +1.98.0 test -p parity` — exit 0
- `parity run` equal-case smoke — exit 0
- `parity run` deliberate mismatch smoke — exit 1 with field-level diff
- Frozen parity cases and fixtures unchanged — verified
- Added-line secret/path scan — clear

## Contract evidence

The harness isolates HOME/XDG/CWD and reserved environment roots, rejects
fixture traversal, captures raw process status/signal/stdout/stderr, performs
bounded stdin and descendant cleanup, and reports non-Unix process-tree support
as an explicit unsupported capability. It records filesystem manifests, raw
HTTP exchange sequences, raw MCP frames, semantic JSON comparisons, and
WAL-aware SQLite snapshots through bundled rusqlite backup support. Normalizers
are typed, allowlisted, reason-tagged and surfaced in diagnostics. The CLI
runner exercises the same comparison path rather than claiming parity from a
placeholder executable.

Residual limitation: non-Unix process-tree cleanup remains explicitly
unsupported until a Windows Job Object implementation is added; the harness
fails closed instead of claiming coverage.
