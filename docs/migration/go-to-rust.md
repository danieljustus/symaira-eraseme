# Go → Rust migration ledger

Single resumption entrypoint. Detailed per-slice write-ups live in
`docs/rust-port/handoffs/`; this file is the state, not the narrative.

- Base: `61ef9ecd` (main, clean)
- Toolchain: go1.27.1, rustc 1.98.0 (oracle capture pinned at go1.26.6, commit `4e582f28`)
- Crates: `symeraseme-core`, `symeraseme-engine`, `symeraseme-cli`, `rust-tests/parity`

## Acceptance gates (all four required per slice)

```
cargo nextest run --workspace
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
go vet ./... && gofmt -l .
```

## Contract

The CLI corpus `rust-tests/parity/cases/cli/behavior.json` (166 recorded Go
cases) is the contract. `is_exact_case()` in
`crates/symeraseme-cli/tests/command_surface.rs` splits it: selected cases are
compared byte-exactly, the rest only assert the deferred stub fails closed.
**Migration progress = cases moved into the selected set**, not files ported.

Selected: 144 · deferred: 22 (as of `61ef9ecd`).

## Remaining deferred cases

| Case id | Subsystem | State |
|---|---|---|
| `operate-init-profile`, `operate-show-profile` | identity/profile | in progress — CLI-015, branch `cli-015-profile` |
| `grant-dry-run`, `operate-grant` | grant/tokens | ready |
| `operate-events-show` | events | ready |
| `operate-manual-tasks-show/-complete/-cleanup` | manual tasks | ready |
| `operate-plan-create/-show/-execute` | campaign planning | ready |
| `operate-generate-dashboard/-report/-rebuttal/-scheduler` | generators | ready |
| `operate-auto-confirm`, `operate-classify-reply` | triage | ready |
| `operate-migrate`, `operate-review`, `operate-run-web-form` | misc | ready |
| `operate-mcp` | MCP stdio server | ready |
| `operate-poll-inbox` | IMAP | blocked — needs the unported transport; do not emulate the Go error string |

## Decisions

- 2026-09-21 — this ledger created; `docs/rust-port/handoffs/` stays the
  per-slice evidence store, not a second tracker.
- Work is dispatched into per-slice worktrees off the integrated revision; the
  coordinator alone edits this file, shared manifests and CI.

## Out of authorization

Go deletion, release publication and production cutover. The Go tree stays
runnable — it is the oracle.
