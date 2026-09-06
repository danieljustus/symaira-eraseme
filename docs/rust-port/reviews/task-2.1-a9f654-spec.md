# Task 2.1 final specification review

- Issue: #804
- Task: 2.1 — Port version and build metadata
- Reviewed SHA: `a9f654dc95c9874caa51bb05de1e256e54b1fc4a`
- Reviewer lane: specification
- Verdict: **PASS**

## Closed findings

- All public surfaces derive from deterministic `CARGO_PKG_VERSION`; poisoned
  compile/runtime `SYMERASEME_VERSION` cannot desynchronize or spoof output.
- Root version short flag matches Go: `-v` works and `-V` is rejected with the
  exact Go-compatible error bytes.
- `version extra` and `version --json extra` return the exact Go-compatible
  stderr and exit code 1; root `--version extra` remains compatible.

## Verification

- Rust 1.98 format/check/Clippy/workspace tests — exit 0
- 8 focused version tests — exit 0
- Direct Go/Rust byte comparisons for all version and negative cases — pass
- Go regression gate — exit 0
- poisoned build/runtime environment test — pass
- scope, lockfile and added-line secret/path checks — pass
