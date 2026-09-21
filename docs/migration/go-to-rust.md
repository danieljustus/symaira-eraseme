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

Selected: 149 · deferred: 17 (as of `04e516b3`).

## Remaining deferred cases

| Case id | Subsystem | State |
|---|---|---|

| `grant-dry-run`, `operate-grant` | grant/tokens | ready |
| `operate-events-show` | events | ready |

| `operate-plan-create/-show/-execute` | campaign planning | in progress — CLI-017, branch `cli-017-plan` |
| `operate-generate-dashboard/-report/-rebuttal/-scheduler` | generators | ready |
| `operate-auto-confirm`, `operate-classify-reply` | triage | ready |
| `operate-migrate`, `operate-review`, `operate-run-web-form` | misc | ready |
| `operate-mcp` | MCP stdio server | ready |
| `operate-poll-inbox` | IMAP | blocked — needs the unported transport; do not emulate the Go error string |

## CI caveat

`Rust / native (${{ matrix.os }})` reports **skipping** on PRs, so a green PR
is not native multi-platform evidence. Native target results remain an open
gate for cutover readiness.

## Known parity defects found but not fixed

- **`manual-tasks list` nested task key order.** Go emits the nested task
  objects in struct order (`id, request_id, broker_id, …`); Rust emits them
  sorted alphabetically. The recorded `operate-manual-tasks-list` case has an
  empty `tasks` array, so the corpus does not catch it. Found during CLI-016;
  belongs to the already-merged `list` slice (#1015). Fixing it means enabling
  `serde_json`'s `preserve_order`. **The green corpus is not proof this row is
  clean.**

## Decisions

- 2026-09-21 — this ledger created; `docs/rust-port/handoffs/` stays the
  per-slice evidence store, not a second tracker.
- 2026-09-21 — CLI-015 merged as #1016 (`cedc0b8a`); CLI-016 as #1017 (`04e516b3`).
- Port PRs carry code only; ledger updates go to main separately.
- The `manual-tasks list` key-order defect is being fixed on `fix/manual-tasks-key-order`.
- Work is dispatched into per-slice worktrees off the integrated revision; the
  coordinator alone edits this file, shared manifests and CI.

## Out of authorization

Go deletion, release publication and production cutover. The Go tree stays
runnable — it is the oracle.
