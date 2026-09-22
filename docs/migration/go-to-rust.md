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

Selected: 163 · deferred: 3 — verified by the focused corpus test on the
integrated HEAD `7d02cc58` (run of 2026-09-22; historical: 158/8 at
`93872f48`, main 160/6 after #1022). The three deferred cases are all
historically classified as blocked. The IMAP classification was stale: TLS and
the MCP handler are implemented; CLI-025 below tracks the missing CLI adapter.
The two llmkit transport cases remain blocked on the shared implementation.

## Open tasks (run of 2026-09-22)

| Task | Cases | Branch | State |
|---|---|---|---|
| CLI-020 | `operate-auto-confirm` + `operate-migrate` (CLI-023) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`) |
| CLI-021 | `operate-review`, `operate-run-web-form` | `rust/cli021-misc` | **merged** — #1022 (`c511736b`) |
| CLI-022 | `operate-mcp` | `rust/cli022-mcp` | **merged** — #1023 (`4c0236fb`) |
| CLI-023 | `operate-migrate` (validateRoots scope) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`); engine behind validation stays fail-closed, see Known defects |
| CLI-024 | migration detection, report, backup/state | `rust/cli024-engine` → `rust/cli024-integration` | **in progress** — real Go corpus and red replay baseline committed as `7e603006` |
| CLI-025 | `operate-poll-inbox` | not dispatched | **ready after CLI-024 integration** — existing IMAP TLS + MCP handler; serialize shared `cli.rs` edits |

## Active continuation (2026-09-22, CLI-024)

- Verified base: `8986a3db3d60d37b89368d37e15f1e98c60672f2`; main clean and
  equal to origin/main at dispatch. Read-only fetch used; unrelated branch
  cleanup and the concurrent release/Windows-fix PRs were left untouched.
- CLI-024 implementation: `rust/cli024-engine`, `.worktrees/cli024-engine`;
  delegated writer `sa-0-04ed9c54` / `deleg_dc132177`. Session-bound, not durable.
- CLI-024 fixtures/integration: `rust/cli024-integration`,
  `.worktrees/cli024-integration`; coordinator owns corpus, replay and this ledger.
- Real pinned Go generator completed with exit 0: 174 CLI cases, including eight
  new migration scenarios; all previous 166 CLI records unchanged. Rust parity
  is **pending**, not 171/3 verified yet. Backup/state filesystem replay added
  against the existing `migration` record; native gates and review pending.
- Next: prove repeat-generation stability, integrate the actual worker commits,
  run full owning crates + workspace lint/fmt/nextest, independently review,
  then obtain required CI before promotion. Go remains runnable; no release.

Loaded this continuation: `guard-repo`, `go-rust-port-parity` plus workflow,
contract-matrix and differential-testing; `code-editing`, `autonomous-coding-agents`,
`parallel-repo-agents`, `evidence-gated-testing`, `port-contract-engineering`,
`python-go-port-parity`, and this skill's worker-dispatch/porting-pitfalls references.

## Prior next action (parked 2026-09-22, second pass — stopped early by user)

**CLI-024 — `migrate` engine** (Detect + dry-run report + mutating path in
`internal/migration/migration.go`, 907 lines): new recorded CLI cases for the
dry-run scenarios (extend `scripts/generate-go-oracle-fixtures.sh`, bump
`expected["cli"]` and `command_surface.rs` 166/163/3 in the same change); the
mutating path is already pinned by the filesystem `migration` case in
`rust-tests/parity/cases/filesystem/manifests.json` (backup/state/manifests +
stdout/stderr sha256) — no Rust replay consumer exists for that case yet, so
wiring the replay is part of the slice. Planned worktree `.worktrees/cli024-*`
@ `305b394b` (git-ignored); worker dispatch prepared but not sent.

Verified 2026-09-22 (second pass): `git diff 4e582f28 HEAD -- internal/migration
internal/scheduler` is empty (fixtures pin current Go behavior); the Rust
engine scheduler already exports `generate`, `detect_legacy_python_unit`,
`detect_platform`; PR #1031 merged as `305b394b` (windows clippy fix). Known
unowned WIP left untouched: detached worktree `.claude/worktrees/determined-lewin-d87b4f`
@ `52e594eb` ("manual-tasks list task objects in Go struct order", clean, not
in main — superseded by #1018; salvage or drop in a later cleanup).

Run of 2026-09-22 closed: integrated HEAD `7d02cc58`, corpus selected
163 / deferred 3, focused corpus test + clippy + fmt + vet + gofmt all
exit 0 on that revision. Ready non-blocked queue: empty — next run has
nothing to start unless an externally blocked row unblocks (llmkit
transports, IMAP transport) or the migrate engine gets its own slice with
Go fixtures.

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
| `operate-migrate` | migration engine | implemented — #1024, validateRoots scope only (engine fail-closed, see Known defects) |
| `operate-review`, `operate-run-web-form` | misc | **merged** — #1022 (selected 160/6) |
| `operate-mcp` | MCP stdio server | **merged** — #1023 (`4c0236fb`) |
| `operate-poll-inbox` | CLI adapter | ready — TLS/STARTTLS merged in #982 (`8f060a36`), MCP handler merged in #991 (`a8c393d5`); only the CLI dispatch is deferred. Residual root-store/UTF-7 differences remain separate contract gaps. |
| `operate-auto-confirm`, `operate-migrate` | triage / migration | **merged** — #1024 (`7d02cc58`) |

## CLI-025 execution notes

The prior "unported IMAP transport" blocker is disproved by current source and
history. `crates/symeraseme-cli/src/mcp/handler.rs::poll_inbox` constructs the real
production dialer, with the source-bound nine-case handler fixture and eleven-case
transport corpus. The next slice must call that handler, not implement IMAP again.
`cmd/symeraseme/extra_commands.go:92-164` supplies the exact CLI contract:
only explicitly changed flags enter the argument map, `--since` and `--since-days`
share one value (last spelling wins), text output prefers a nonempty `message`,
otherwise prints `success`. Preserve these differences from other thin wrappers.
The existing `operate-poll-inbox` recording exercises a real refused local TCP
connection, not a transport-emulation string. The selected/deferred counters may
only move after that native call matches its recorded output.

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
  `validateRoots` stat error).
- 2026-09-22 — CLI-023 scoped as "port `validateRoots` for real, fail closed
  behind it": the validation walk (absolute/clean paths, symlink-component
  rejection, Go's wrapped `lstat` text) is a genuine port under the recorded
  differential; only the detection/report engine stays an explicit
  not-implemented branch. This supersedes the earlier "error-path-only would
  be emulation" verdict — emulation would be copying the error string without
  the validating code, which this is not. Landed in #1024 (`66c734af`).
- 2026-09-22 — Rust's `target_os` for Go's `runtime.GOOS == "darwin"` is
  spelled `macos`; a `#[cfg(target_os = "darwin")]` allow-list compiles
  clean and silently never fires (found via the recorded `/tmp` symlink
  component). Go platform guards port as `cfg(target_os = "macos")`.
- 2026-09-22 — Rust CI gates a 90 % line coverage on `mcp/**`-matching
  files (`rust-ci.yml` critical set); subprocess corpus replays are not
  llvm-cov-instrumented, so new handler/serve code needs in-process tests.
  Local replication: `cargo llvm-cov --workspace --all-features --json`
  plus the workflow's file filter — #1023 failed at 89.65 % and passed at
  90.43 % after its stream tests.
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
  (target selected 163 / deferred 3 — the third deferred slot after the
  three blocked llmkit/IMAP cases is `classify-reply`, also llmkit).
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
