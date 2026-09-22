# Go → Rust migration ledger

Single resumption entrypoint. Detailed per-slice write-ups live in
`docs/rust-port/handoffs/`; this file is the state, not the narrative.

- Base: `b39606b8` (main, clean; supersedes `861527cf`)
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

Selected: 158 · deferred: 8 (as of `93872f48`).

## Open tasks (run of 2026-09-22)

| Task | Cases | Branch | State |
|---|---|---|---|
| CLI-020 | `operate-auto-confirm` only (re-scoped) | `rust/cli020-triage` | implemented — PR #1024 (`7cdf266f`), asserts 161/5 |
| CLI-021 | `operate-review`, `operate-run-web-form` | `rust/cli021-misc` | **merged** — #1022 (`c511736b`), main asserts 160/6 |
| CLI-022 | `operate-mcp` | `rust/cli022-mcp` | implemented — PR #1023 (`0fb1f5e2`), rebased onto #1022, asserts 161/5 |
| CLI-023 | `operate-migrate` | — | ready — re-scoped from CLI-021: the recorded case only exercises `validateRoots`' stat error and the migration engine (`internal/migration`, 907 lines) is unported; needs its own slice with Go fixtures for the dry-run/report paths |

Integration note: CLI-020 and CLI-022 both base on the post-#1022 main and
each assert 161/5 (different cases); whoever merges second rebases and
reconciles the counts to 162 selected / 4 deferred.

Wave 1 note: all three wave-1 workers died on HTTP 429 (Codex quota, ~9 h
reset) after ~13 s with no commits; the coordinator implemented every slice
directly in the slice worktrees, per the dispatch contract. Treat an empty
worktree/branch as quota loss, not a failed slice.
Loaded this session (do not re-load): `go-to-rust-migration`,
`go-rust-port-parity` + `references/workflow.md`, `guard-repo`,
`autonomous-coding-agents`, `parallel-repo-agents`,
`go-to-rust-migration/references/worker-dispatch.md`.

## Remaining deferred cases

| Case id | Subsystem | State |
|---|---|---|


| `operate-generate-dashboard/-report/-scheduler` | generators | done — #1021 |
| `operate-generate-rebuttal`, `operate-classify-reply` | LLM | blocked — `internal/llm` reports transports as not ported (`llmkit` owns the `auth_failure` text in Go); emulating it is forbidden |
| `operate-migrate` | migration engine | ready — CLI-023 (engine unported; see task table) |
| `operate-review`, `operate-run-web-form` | misc | **merged** — #1022 (selected 160/6) |
| `operate-mcp` | MCP stdio server | implemented — #1023 (asserts 161/5) |
| `operate-poll-inbox` | IMAP | blocked — needs the unported transport; do not emulate the Go error string |
| `operate-auto-confirm` | triage | implemented — #1024, no-reply branch only (asserts 161/5) |

## CI caveat

`Rust / native (${{ matrix.os }})` reports **skipping** on PRs, so a green PR
is not native multi-platform evidence. Native target results remain an open
gate for cutover readiness.

## Pinned as measured, not desired

- **`plan execute --dry-run` on a web-form request appends a `SENT` event.**
  Go's `executeWebformRequest` gates only the adapter on `dry_run`, not the
  event append (the early return at `internal/campaign/execution.go:100`
  applies only when the runner is nil, and the CLI supplies one). It sends
  nothing, but it is not read-only, and the recorded `SENT` removes that
  request from the next batch. Changing it is a contract change (CLI-017).

## Known parity defects found but not fixed

- **Workspace-guard edge strings.** Root-open failures still print
  `workspace root is unavailable` where Go wraps the cause
  (`resolve workspace root: …` / `workspace file read failed`), and a
  cap-std `InvalidInput` open failure maps to `ErrPathInvalid`'s text where
  Go says `workspace file read failed`. Neither path is recorded in the
  corpus; both live in `redaction/path.rs`.
- **MCP HTTP transport (CLI-022).** `mcp`/`serve` without `--stdio` stays on
  the deferred fail-closed stub; Go binds the port and writes an auth
  token file. Not recorded in the corpus.
- **MCP malformed-stream text (CLI-022).** A value cut off at EOF aborts
  with `malformed JSON value at byte N` instead of `encoding/json`'s
  `unexpected EOF`, and a malformed value mid-stream waits for the next read
  where Go errors immediately. Unrecorded paths; see the `ponytail` comment
  in `mcp/stream.rs::serve_stdio`.
- **`auto_confirm` with a stored reply (CLI-020).** Fails closed with an
  explicit message where Go runs `confirmation.AutoConfirm` (browser
  subsystem unported). The recorded case is the no-reply branch.
- **`go_map_order` exemption (CLI-020).** `ToolHandler::call` sorts every
  result except `auto_confirm` (Go structs keep declaration order). Any
  future Go-struct-returning tool needs the same exemption — grep the
  comment in `mcp/handler.rs`.

## Fixed parity defects (folded into the corpus or a regression test)

- **`manual-tasks list` nested task key order.** Fixed by #1018
  (`a96d65d5`): `serde_json` now builds with `preserve_order`, ported
  structs keep declaration order, and every payload Go builds from a
  `map[string]any` is sorted explicitly via `jsonorder::go_map_order`.
- **`preserve_order` interaction with `plan execute` / `plan show`.**
  Found while rebasing #1018 onto the CLI-017 tree: both payloads are Go
  maps, so insertion order diverged. Fixed in the same merge
  (`execute_campaign` sorts recursively; `plan show` sorts its document).
  Lesson: any `preserve_order` consumer must classify each payload as
  struct-ordered or map-ordered against the Go source — the corpus only
  catches the cases it records.

## Decisions

- 2026-09-22 — all three wave-1 worker results came back HTTP 429 (Codex
  quota); the coordinator implemented CLI-020/021/022 itself in the slice
  worktrees. Dispatch failure is not a slice outcome.
- 2026-09-22 — `classify-reply` re-scoped out of the triage slice to
  `blocked` (llmkit transports): its recorded bytes are an `auth_failure`
  chain owned by `corekit/llmkit`, which Rust's `llm` module deliberately
  reports as not ported.
- 2026-09-22 — `ToolHandler::call` exempts `auto_confirm` from
  `go_map_order`: Go sorts map serialization but emits structs in field
  order; the recorded `confirmation.Result` bytes proved the blanket sort
  wrong.
- 2026-09-22 — CLI-021 re-scoped: `review` + `run-web-form` landed as #1022
  (`c7570432`, asserts 160/6); `operate-migrate` became CLI-023 (engine is
  907 unported lines and its single recorded case pins only the
  `validateRoots` stat error — an error-path-only port would be emulation).
- 2026-09-22 — the differential replay now substitutes the recorded
  `<ORACLE_ROOT>` in argv and folds the runtime root back out of stdout and
  stderr (byte-level), restoring the capture's normalization; before this,
  no path-bearing case could replay byte-exactly, in Go or Rust.
- 2026-09-22 — a failed dispatch on HTTP 429 (Codex quota) is not a slice
  outcome; the coordinator implemented CLI-021 in its worktree, per the
  dispatch contract.
- 2026-09-22 — wave 1 of this run dispatched three writers in parallel
  (CLI-020/021/022) with disjoint subsystem scopes but a known shared-edit
  surface (`cli.rs` dispatch arms + `command_surface.rs` selection/counts);
  the coordinator merges serially and owns the final count reconciliation
  (target selected 164 / deferred 2 once all three land).
- 2026-09-21 — this ledger created; `docs/rust-port/handoffs/` stays the
  per-slice evidence store, not a second tracker.
- 2026-09-21 — CLI-017 merged as #1019 (`c0573f86`).
- 2026-09-21 — CLI-015 merged as #1016 (`cedc0b8a`); CLI-016 as #1017 (`04e516b3`).
- 2026-09-21 — key-order fix merged as #1018 (`a96d65d5`); CLI-018
  (events/grant) merged as #1020 (`861527cf`). Selected 155 / deferred 11.
- 2026-09-21 — CLI-019 (generate-dashboard/-report/-scheduler) merged as
  #1021 (`93872f48`). Selected 158 / deferred 8. `operate-generate-rebuttal`
  stays deferred: the LLM subsystem is unported and its auth-failure string
  must not be emulated. Delegated worker for this slice died on Codex 429
  (fallback did not trigger); the coordinator implemented the slice directly
  in the slice worktree.
- Port PRs carry code only; ledger updates go to main separately.
- Work is dispatched into per-slice worktrees off the integrated revision; the
  coordinator alone edits this file, shared manifests and CI.

## Out of authorization

Go deletion, release publication and production cutover. The Go tree stays
runnable — it is the oracle.
