# Go → Rust migration ledger

Single resumption entrypoint. Detailed per-slice write-ups live in
`docs/rust-port/handoffs/`; this file is the state, not the narrative.

- Integrated base: `afa22cb9543cc53a9f3de3c1d05ba977e4a1cf75`, branch `rust/cli024-integration`; JSON/review repairs are uncommitted. Shared main remains clean at `8986a3db`.
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

The CLI corpus `rust-tests/parity/cases/cli/behavior.json` (174 recorded Go
cases) is the contract. `is_exact_case()` in
`crates/symeraseme-cli/tests/command_surface.rs` splits it: selected cases are
compared byte-exactly, the rest only assert the deferred stub fails closed.
**Migration progress = cases moved into the selected set**, not files ported.

Local selected: **171 · deferred: 3**, executed on `afa22cb9` by the complete
workspace nextest gate on 2026-09-22. All 166 previous CLI records are unchanged;
eight migration records were added. Last merged evidence remains 163/3 at
`7d02cc58` (historical: 158/8 at `93872f48`, 160/6 after #1022).
A green recorded CLI corpus does not close the separate JSON-state, review or native gates. The three deferred CLI cases are IMAP/LLM, not migration cases.
The IMAP classification was stale: TLS and the MCP handler are implemented;
CLI-025 below tracks the missing CLI adapter. The two llmkit transport cases
remain blocked on the shared implementation.

## Open tasks (run of 2026-09-22)

| Task | Cases | Branch | State |
|---|---|---|---|
| CLI-020 | `operate-auto-confirm` + `operate-migrate` (CLI-023) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`) |
| CLI-021 | `operate-review`, `operate-run-web-form` | `rust/cli021-misc` | **merged** — #1022 (`c511736b`) |
| CLI-022 | `operate-mcp` | `rust/cli022-mcp` | **merged** — #1023 (`4c0236fb`) |
| CLI-023 | `operate-migrate` (validateRoots scope) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`); engine behind validation stays fail-closed, see Known defects |
| CLI-024 | migration detection, report, backup/state | `rust/cli024-integration`; JSON follow-up `rust/cli024-json` | **in progress** — base `afa22cb9` plus uncommitted repairs; static review, macOS workspace and focused native Linux gates pass; native Windows and exact-head CI remain open |
| CLI-025 | `operate-poll-inbox` | not dispatched | **pending CLI-024 acceptance** — existing IMAP TLS + MCP handler; serialize shared `cli.rs` edits |

## Active continuation (2026-09-22, CLI-024)

- Mode/status: execute / local verification complete; native CI/publication pending.
  Coordinator: `.worktrees/cli024-integration`, `rust/cli024-integration`.
  Dirty candidate is based on `afa22cb9`; reviewed input manifest is
  `target/cli024-review-repair-inputs.json`, SHA-256
  `935002f88832172518df1179ed194e3e700ff5800a066935885ba97aa6269488`.
  Source/test/oracle inputs match the completed static verdict. Shared main remains
  clean at `8986a3db`; no remote publication, Go deletion, release or cutover.
- Reconciled results: `proc_531054572d25` returned **CHANGES_REQUIRED** for
  Windows case-aliased overlap, Windows readonly loss, and scheduler error
  context. Repairs and regression tests are implemented, not yet accepted
  natively. `proc_6981abee04ad` delivered the JSON decoder and genuine 500-case
  production-Go oracle; the coordinator transferred 11 new files byte-identically
  and applied four scoped integration hunks, preserving the safety repairs.
- Local focused evidence: all four generated/native scheduler failure scenarios
  match immutable Go `4e582f28`; all Unicode scalars match the Go 1.26.6 fold
  capture (1454 mappings, 210 exact ranges). JSON writer evidence and its raw
  observations are retained in `rust-tests/parity/oracle/migration-state/`.
  Parent engine tests passed; the first aggregate stopped on a test-only
  non-octal permission literal, now corrected. Do not inherit the old clean-base
  428-test result as final-tree acceptance.
- Final macOS aggregate `proc_cd6f87dde9ab`: **exit 0**, 432 workspace tests
  executed/passed, two child-entry helpers skipped; fmt, strict all-target/all-
  feature Clippy, fresh 500-case Go JSON check and diff validation passed.
  Both safety check entrypoints accepted valid copies and rejected corrupted
  copies without rewriting them. Doctests selected zero tests. The earlier
  mutation-copy Git-prefix error was corrected with a disposable local shared
  clone; no product/fixture expectations changed. All 1539 reviewed input hashes
  still matched after this run.
- Final review `proc_650668909fd5` completed with **CHANGES_REQUIRED**:
  its 1539-file manifest matched before/after; prior production findings were
  resolved statically, but the Windows readonly regression checked the wrong
  backup path. The sole subsequent source/test change is that assertion at
  `migration_review_regressions.rs:144`, now relative to `report.backup_dir`
  under `source/config.toml`. Production and oracle bytes are unchanged.
  Bounded follow-up review `proc_95d4e6d971c2` is **PASS_STATIC**, with all
  1539 input hashes checked before/after and no unresolved static findings.
  Fresh macOS aggregate `proc_a256a9044825` also exited 0: 432 tests passed,
  two helpers skipped, strict workspace Clippy and fmt passed. Log:
  `target/cli024-after-review.log`. This is post-test-repair evidence, not
  inherited approval from the earlier candidate.
- Focused native Linux attempt 4 is **PASS**. Earlier attempt 3
  (`proc_b0b109a67d51`, `target/cli024-linux-3/run.log`) identified **EMFILE**
  in unchanged `symeraseme-core/build.rs:47`: the container's measured soft
  descriptor limit is 1024, while registry collection retains 1277 broker
  handles plus other assets/traversal handles. Tracked as GitHub issue #1034;
  no production change or weakened source-safety check was made.
  Attempt 4, `proc_46cc9db786d5`, uses only a container-local
  `--ulimit nofile=8192:8192` adjustment, still offline and non-root. Its
  source archive, runner copy, identity and full log are retained separately
  under `target/cli024-linux-4/`. Prior failed artifacts remain untouched.
  It exited 0 on Linux aarch64 with Rust 1.98.0: five tests passed across four
  test binaries, including all 500 declared JSON observations, distinct actual
  non-UTF-8 backup names, four scheduler failure observations and all Unicode
  scalars. The old engine first reproduced the expected scheduler-context
  failure; the restored candidate passed. All 2035 archived input hashes were
  checked; all 1539 review inputs match the live candidate. Only the two
  non-build handoff documents changed after capture. This is a targeted Linux
  runtime gate, not the complete native workspace/CLI matrix.
  Local Windows cross-check failed in `ring` because `assert.h` is unavailable,
  before checking engine target code; **native Windows remains unverified**.
  No owned review/test process remains pending. Next external gate requires a
  committed/published candidate and `rust-ci.yml` workflow dispatch on that exact
  head; its native job runs on Linux/macOS/Windows but is skipped on PRs. A PR
  supplies the separate fast/security/coverage gates. Publication authorization
  has not been inferred from these completion notices. No merge/release is
  authorized here. CLI-025 remains dependency-gated.
- Delayed notifications reconciled: Docker pull `proc_ae3212f3572a` succeeded
  (image digest retained in the Linux identity); `proc_4815d0401e2e` is failed
  Linux attempt 2, not a new run. Engine worker `proc_068e417c7478` exited 0;
  its retained commit `474751db07bc68ef579547a012cf338301c28b8d` is already
  integrated as `afa22cb9`. Do not re-integrate or restart that worker.
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

- **Migration review/native acceptance (CLI-024, local dirty candidate).**
  JSON state/completion decoding and all review findings are repaired and
  statically accepted against manifest `935002f8`. The post-repair 432-test
  macOS workspace gate and focused five-test Linux gate pass. These do not
  substitute for native Windows runtime evidence or exact-head CI. The
  low-descriptor-limit registry build failure remains separately tracked in
  #1034; raising the test-container limit is not a production fix.
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
