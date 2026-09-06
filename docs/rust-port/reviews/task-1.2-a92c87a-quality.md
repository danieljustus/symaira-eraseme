# Task 1.2 quality and security review

- Issue: #803
- Task: 1.2 — Add the neutral differential harness
- Reviewed SHA: `a92c87a91c3058da6e4c7a183ef8cbf1ab1975d6`
- Reviewer lane: quality/security
- Verdict: **APPROVED**

## Verified gates

- `cargo deny check` — exit 0; advisories, bans, licenses and sources pass
- `cargo +1.98.0 fmt --all --check` — exit 0
- `cargo +1.98.0 check --workspace --all-targets` — exit 0
- `cargo +1.98.0 clippy --workspace --all-targets -- -D warnings` — exit 0
- `cargo +1.98.0 test --workspace --all-targets` — exit 0
- 21 parity tests — exit 0
- Equal and deliberate-mismatch CLI smoke cases — expected exits 0 and 1
- Bounded output overflow smoke — explicit fail-closed exit 2
- Frozen fixture scope and added-line secret/path scans — clear

## Security evidence

The harness bounds subprocess output, stdin cleanup, HTTP framing and mock
shutdown; uses opaque private sandboxes with explicit Unix permissions; rejects
fixture/database traversal and symlink components; creates and cleans bundled
SQLite backup targets safely; and emits structural redacted diagnostics rather
than raw JSON, database contents or personal paths. Unix process-group cleanup
uses a narrowly scoped FFI boundary; non-Unix process-tree support remains an
explicit unsupported capability and never claims coverage.

Informational residuals: cargo-deny reports unused baseline license allowances,
and Windows Job Object support is not implemented. Neither blocks the current
shadow harness because both are explicit and fail closed.
