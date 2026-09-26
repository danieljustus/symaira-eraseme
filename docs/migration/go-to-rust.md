# Go → Rust migration ledger

Single resumption entrypoint. Detailed per-slice write-ups live in
`docs/rust-port/handoffs/`; this file is the state, not the narrative.

- Integrated status (2026-09-24): the proof branch landed on GitHub `main` as `3f133e75`. Its exact-head Go CI, Rust CI (including the native OS matrix), general CI and CodeQL completed successfully. No release, cutover, Go removal or paid provider is authorized. The older slice notes below are historical; a green integrated CI run does not prove a retained older Go rollback binary can read schema v2 (see #1035).
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

The CLI corpus `rust-tests/parity/cases/cli/behavior.json` (175 recorded Go
cases) is the contract. `is_exact_case()` in
`crates/symeraseme-cli/tests/command_surface.rs` splits it: selected cases are
compared byte-exactly, the rest only assert the deferred stub fails closed.
**Migration progress = cases moved into the selected set**, not files ported.

Local selected: **175 · deferred: 0** on the integrated local tree. The real
`poll-inbox` adapter, invalid `--since`, and both reply-triage commands replay
source-bound Go cases. The LLM-backed cases reproduce Go's missing-key error
without a paid call. A
green CLI corpus does not close JSON-state, review or native platform gates.

## Integrated continuation (2026-09-23)

- CLI-024 migration engine and its 500-case Go JSON oracle are committed.
  Focused macOS and Linux aarch64 tests passed; native Windows and an exact-head
  remote matrix remain open. The earlier 432-test workspace result applies to
  that CLI-024 candidate, not automatically to this later integrated head.
- CLI-025 polls a real IMAP adapter with 175 recorded Go CLI cases, all selected
  after the LLM transport integration. Filesystem generators, populated manual tasks,
  events and grants, and source-bound malformed MCP stdio/process cases are
  integrated and exercised on macOS.
- MCP HTTP starts a real Rust server with bearer auth, Origin and bind policy,
  body limits, token rotation and signal drain. The integrated macOS
  `mcp_http_process` plus `mcp_stdio_process` gate passed 12 tests; native
  Linux/Windows process behavior remains open.
- The Swift app's opt-in Rust process test launches the HTTP binary, checks an
  unauthenticated 401, token-backed tools/list and `list_brokers` calls, and
  verifies shutdown. This exposed two existing app response-shape defects,
  fixed in `026c3839`. The Swift suite passed 41 tests with the Rust binary.
- IMAP modified UTF-7 mailbox names and outbound MIME bytes replay pinned Go
  fixtures. Default IMAP TLS now loads platform roots, including Windows
  LocalMachine ROOT; the follow-up passed independent static review and five
  focused macOS IMAP tests at `c7954258`. Native Windows chain policy and a
  custom macOS Keychain root remain unverified. No paid LLM or SMTP provider
  was called.
- The four Go tick histories now replay through Rust scan, apply, persisted
  scheduler events and projections (`efc718b`); the two Go conformance tests
  and four Rust tick-apply tests pass. Core manual-task HandleList/Show/Complete/
  Cleanup orchestration is integrated at `96da417f`, independently reviewed,
  with eight source-bound Go handler cases and nine focused integrated Rust
  tests passing. Positive tick CLI actions now retain Go's capitalized struct
  keys and declaration order (`ef8ba9c`); matching Go/Rust wire tests pass.
  Native filesystem evidence remains open.
- The offline release checker at `4fa11368` verifies six current Go snapshot
  archives, exact root files, member uniqueness/type and SHA-256 entries.
  Four malformed-archive controls and the real snapshot pass. This establishes
  the existing package contract; the release workflow still packages Go.
  Offline Cargo audit/deny and a source CycloneDX inventory passed, while Rust
  release artifacts, artifact SBOM/provenance, signing and notarization remain
  unverified.
- Local Rust archive staging now produces the six required archive names,
  root members and checksums from explicit caller-supplied binaries (`1f63e692`).
  Its placeholder-binary test passes the offline verifier; native Rust binary
  format, runtime, signing and publication are not established by that test.
- The manual `.github/workflows/rust-prerelease.yml` gate now builds on native
  macOS, Linux and Windows arm64/amd64 runners, checks binary startup/linkage,
  stages the six Rust binaries with the legacy archive contract, and verifies
  the resulting archives and checksums. Until that workflow runs successfully,
  REL-001..004 remain partial; `release.yml` still packages Go and is unchanged.
- Windows release builds select Rust's static CRT feature and fail if `dumpbin`
  reports a dynamic CRT or non-system DLL dependency. The Go release contract
  sets `CGO_ENABLED=0`; no Windows native workflow run has verified the Rust gate yet.
- Core `GetPlan` now serves both CLI and MCP reads. The independently reviewed
  source-bound Go campaign oracle at `131fb6b4` checks all seven pinned plan
  timestamps before and after execution, plus local no-send web-form/manual
  fallback and missing-email-sender transitions. Three integrated Rust tests
  passed. MCP `execute` now has isolated local replay; broader execution paths
  remain open.
- `review` now has eight live Go/Rust CLI differential cases for positional and
  `--path` aliases, text/JSON modes and failures (`fe9250a`); the input file is
  unchanged and four successful paths must redact its address.
- Core reply classification and rebuttal service orchestration is integrated
  (`4af87d9d`). A source-hash-pinned Go oracle compares full results, saved
  reply columns, ordered event records and stable projections with injected
  local clients; 19 focused Rust tests passed after integration. Both CLI
  wrappers replay 15 local-agent Go cases, including save flags, aliases and
  errors (`23a44ff`, `e1450c5`). MCP `classify_reply` and `generate_rebuttal`
  replay the live Go handler through the Rust stdio process (`f690b8bd`). Four
  llmkit-backed chat providers now use Rust transports with five fake-local
  HTTP Go cases (`d355e3b2`); echoed keys are redacted from errors and Debug
  output (`17607327`). No paid provider was used. Native target runs remain open.
- The real Rust MCP stdio process now replays 72 valid Go initialize ID/params
  cases as adjacent JSON values (`7cc76c8a`) with bounded I/O and exact output;
  its focused integrated test passed. Ten malformed/truncated cases remain
  source-bound. Six malformed parse cases and four size/depth boundaries also
  replay Go 1.26.6 process bytes (`6ccd1a4`); broader fuzzing remains open.
- MCP scheduler install, status and uninstall now replay source-bound Go cases
  through the real Rust stdio process (`c485e852`). Cron runs against a private
  crontab; launchd and systemd installs use isolated HOME roots and fake service
  commands. All five integrated stdio process tests passed. Native target runs
  and more scheduler failure paths remain open.
- MCP `plan_create` and `execute` now replay five source-bound Go cases through
  the real Rust stdio process (`aea9f15`). The cases cover the default and a
  corrupt explicit profile, dry-run preview, denied live execution, and a
  consented local manual fallback without a network sender. The first
  integrated replay exposed Go struct field ordering in the content text; the
  focused stdio process suite then passed 6/6 and the CLI package passed 96/96.
  Consent directory resolution now uses `USERPROFILE` on Windows and propagates
  missing-home errors; native Windows execution remains unverified.
- Reporting JSON and HTML now have raw Go byte oracles on isolated stores
  (`29d48317`). The zero-campaign CLI artifact, one-campaign export, and
  two-campaign export with an empty first campaign cover the HTML boundaries;
  the focused integrated core tests passed 6/6 and filesystem replay passed 1/1.
- A new PlanCampaign/GetPlan oracle (`956bd9db`) compares raw Go/Rust result,
  request, campaign and persisted event bytes. It exposed Go's sorted JSON map
  order in `payload_json`; the shared event append now matches it (`c0d6dd19`).
  The focused integrated plan tests passed 3/3.
- The integrated tree at `29d48317` passed 481 workspace tests (two helper
  tests skipped) with `TMPDIR=/tmp`; `go vet ./...` and `cargo fmt --all --check`
  passed. A preceding run with a longer temporary path hit the macOS Unix
  socket path limit in `identity_profile`; that test passed with `/tmp`.
- The later integrated tree at `79f309f` passed 491/491 macOS workspace tests
  (two child-entry helpers skipped) with `TMPDIR=/tmp`. A test-only host-agent
  deadline was raised after one load-dependent fake-process timeout; its
  focused replay passed. At `20ef924`, `go vet ./...`, tracked-Go `gofmt -l`,
  `cargo fmt --all --check` and strict offline workspace Clippy passed. A
  macOS arm64 Rust release binary built at that head, ran `version`, and linked
  only Apple system frameworks/libraries; the other five native release targets
  and exact-head target matrix remain open.
- On native Linux aarch64, the offline `20ef924` workspace check, 176 Rust
  unit tests (one ignored), frozen 175-case CLI replay and focused LLM tests
  passed. The later `a022c169` fake-sender campaign oracle passed 1/1. That
  container had cached Go 1.26.8 rather than the pinned 1.26.6 and lacked
  rustfmt/Clippy; it is scoped Linux evidence, not a complete final-head
  matrix. Logs are under `target/linux-aarch64-latest-20ef924.log` and
  `target/linux-aarch64-campaign-a022.log`.
- The source-bound campaign oracle now also executes a local two-request
  batch: one fake email send fails and persists `SEND_FAILED`, the next
  succeeds and persists `SENT`. The raw Go/Rust results, events and projections
  match at `a022c169`; the focused macOS test passed 1/1. Real provider and
  broader CLI wrapper behavior remain open.
- MCP `auto_confirm` now reads the newest stored reply, previews a trusted
  link on dry-run, and creates the Go-equivalent durable manual task when a
  clicker is unavailable (`17daf2c5`). A source-bound Go MCP process oracle
  and the focused Rust test compare the response and SQLite effects, including
  a NULL-snippet/no-link failure note. No browser click or network request is
  attempted. The production CLI and MCP campaign callers keep the newly
  injectable fake email sender unset.
- An offline formula gate checks the current separate Homebrew tap formula's
  four versioned macOS/Linux archive URLs, SHA-256 syntax, install command and
  version test; Ruby syntax also passes. It cannot verify hashes against Rust
  release artifacts or perform a local install while those artifacts do not
  exist.
- Final local macOS gate on `7151138`: 491/491 Rust workspace tests passed
  (two helpers skipped); strict all-target/all-feature Clippy and Rust format
  passed; full `go test ./...`, `go vet ./...` and tracked-Go format passed.
  The opt-in Swift suite ran against that exact Rust debug binary and passed
  41/41 tests, including the live HTTP/auth/shutdown integration case. The
  release build on the same commit produced a Mach-O arm64 executable,
  `version` returned `symeraseme 0.13.0`, and `otool -L` showed only Apple
  system frameworks/libraries. Logs: `target/macos-nextest-7151138.log`,
  `target/go-test-7151138.log`, `target/swift-test-7151138.log`, and
  `target/macos-release-7151138.log`. These runs do not satisfy the missing
  native Windows or complete exact-head Linux matrix.
- The `7151138` dependency gate passed offline: `cargo audit --no-fetch`
  scanned both Cargo lockfiles, `cargo deny --frozen check all` passed, and
  Syft produced a CycloneDX 1.7 source inventory with 1,307 components at
  `target/sbom-source-7151138.cdx.json`. Artifact SBOM and provenance remain
  unverified without the complete native Rust archive matrix.
- Three additional local-only Go HTTP observations now cover LLM 429 retry
  exhaustion, malformed success JSON and empty choices (`3d74a200`). The
  integrated Rust transport test passed 5/5; no paid provider was contacted.
- CLI and MCP now open the configured encrypted store and explicitly close it,
  propagating finalization failures (`a4757a66`, `60a1656a`). The two sandboxed
  macOS arm64 switchback runners each passed six steps with a Go 1.26.6 binary
  and the integrated Rust release binary: Rust read existing plain/V3-encrypted
  state, wrote one request, and Go read the post-Rust state. The encrypted run
  also read through Rust MCP stdio and retained its V3 envelope. Evidence and
  artifact hashes are under `/tmp/eraseme-plain-switchback-20260923-05` and
  `/tmp/eraseme-encrypted-switchback-20260923-06`;
  `rust-tests/parity/{plain,encrypted}_store_switchback.py` is the executable
  gate. Native Linux/Windows and real restore/cutover remain open.
- Current integrated code candidate `49eb79a8` passed 493/493 macOS arm64
  workspace tests (two child-entry helpers skipped), strict all-target/all-
  feature Clippy, Rust format, Go 1.26.6 `go test ./...` and `go vet ./...`,
  tracked-Go format, and all 41 Swift app tests against the Rust debug binary.
  The six-step plain and six-step encrypted Go→Rust→Go switchbacks passed again
  using the current Rust release binary and pinned Go 1.26.6; reports are in
  `/tmp/eraseme-{plain,encrypted}-switchback-49eb79a`. Logs are in
  `/tmp/eraseme-{nextest,clippy,go-test,go-vet,swift}-49eb79a.log`.
- On that code candidate, both macOS arm64 and x86_64 Rust release binaries
  launched with `symeraseme 0.13.0` and linked only Apple system libraries;
  the x86_64 SQLite portability smoke passed under Rosetta. The arm64 binary
  SHA-256 is `d3ea067a53e63916bd689c99efa5f7224f670dce668f167abea861c898de1043`;
  x86_64 is `c18f401c9d22ced554c717346ae4a200523d1a3764be8b862f9af8437397a86b`.
  Windows GNU workspace all-target check and strict Clippy passed by cross
  compilation after a two-line Windows lint repair. These are not native
  Windows runtime tests. Offline Cargo audit (both lockfiles), deny, the
  six-placeholder archive gate, and current Homebrew formula syntax/contract
  passed; no Rust release archive matrix or publication was produced.
- The earlier parallel macOS x86_64 run at `49eb79a8` hit two fixed 10-second
  process deadlines under load. After serializing the suite and building Go
  oracles for the Rust target architecture, the full `34fe0ef3` run passed
  493/493 under Rosetta (two helpers skipped); see
  `/tmp/eraseme-nextest-macos-x86-serial-34fe0ef.log`. Rosetta is executable
  x86_64 evidence, not a native Intel host.
- Current integrated code `f7f6f72c`: the full macOS arm64 workspace suite
  passed 496/496 (two helpers skipped), including source-bound local 403 and
  404 LLM failures and six fixed-clock MCP reporting cases. The 403/404
  fixtures regenerated byte-for-byte with Go 1.26.6. `go test ./...`,
  `go vet ./...`, Rust format, strict all-target/all-feature Clippy and the
  production Go coverage gate passed; coverage is 78.22% (6,939/8,871).
  Logs: `/tmp/eraseme-nextest-f7f6f72c.log`,
  `/tmp/eraseme-go-{test,vet,coverage-prod}-19c20f50.log` and
  `/tmp/eraseme-clippy-19c20f50.log`. The MCP clock oracle runs from an
  isolated temporary profile; a hostile inherited data path and project
  config remain untouched. The complete macOS x86_64 suite under Rosetta
  also passed 496/496 (two helpers skipped) at `f7f6f72c`, serialized to
  avoid load-dependent process deadlines; log:
  `/tmp/eraseme-nextest-macos-x86-f7f6f72c.log`. A later metadata-only
  repair (`92a573ba`) changed the scheduler/campaign oracle source revision
  from an isolated-worktree commit to the byte-identical, reachable main
  commit `3b61859f`; no fixture case or side effect changed. Its focused
  scheduler/campaign source-bound replay passed 2/2 at `92a573ba`.
  The Swift app suite passed 41/41 with the Rust debug binary, and strict
  macOS x86_64 and Windows GNU all-target Clippy passed. Logs:
  `/tmp/eraseme-mcp-source-revision-92a573ba.log`,
  `/tmp/eraseme-swift-92a573ba.log`, and
  `/tmp/eraseme-{clippy-macos-x86,windows-clippy}-92a573ba.log`.
  On exact `92a573ba`, native Linux aarch64 passed 497 workspace tests across
  55 suites (zero failures, two intentional ignores), strict Clippy, Go/Rust
  format, `go test ./...`, `go vet ./...` and production Go coverage: 78.16%
  (6,934/8,871; 75% gate). The debug CLI launched as AArch64 ELF version
  0.13.0 with resolvable system libraries. Logs are under
  `/tmp/symeraseme-native-gate/logs/` with `92a573ba` in each filename.
  Native Windows and Linux amd64 remain unavailable without runners for this
  unpublished checkout; Rosetta does not replace a native Intel macOS host.
  No release, cutover or publication is authorized.
- The six-step plain and six-step encrypted macOS arm64 Go→Rust→Go
  switchbacks passed again using a Go 1.26.6 CLI built from `92a573ba`
  and the existing Rust release binary. Rust production sources are unchanged
  since that binary was built at `49eb79a8`. The disposable runners retained
  their source/artifact identities and step evidence in
  `/tmp/eraseme-{plain,encrypted}-switchback-92a573ba/report.json`;
  neither run touched a production store or performed a cutover.
- Linux aarch64 disposable switchbacks now pass six plain and six encrypted
  Go→Rust→Go steps using Go 1.26.6 and Rust 1.98.0 binaries built from clean
  `d333ed84` (production code unchanged from `92a573ba`). The Linux runner
  confines each CLI/probe child with private mount, network and PID namespaces,
  read-only host shares, Landlock ABI 4, a dropped uid and no-new-privileges.
  Seven focused controls pass, including a no-namespace fail-closed case;
  all eight runtime denial probes pass. The host-share (`virtiofs`) and
  guest-local (`ext4`) write markers remained byte-identical and were removed.
  The source/build manifest and copied reports are in
  `/tmp/symeraseme-native-gate-host/switchback-linux-d333-logs/`.
  On the integrated `cb21489` runner, macOS plain and encrypted switchbacks
  passed six steps each in `/tmp/eraseme-{plain,encrypted}-switchback-b6a43136-r2/`;
  its focused controls passed six cases with the Linux-only case skipped.
  The one-line macOS probe fix changed no Linux behavior; Linux focused
  controls passed again after that fix. Native Windows switchbacks and a
  real user-data restore remain open; no production cutover occurred.

## Open tasks (run of 2026-09-22)

| Task | Cases | Branch | State |
|---|---|---|---|
| CLI-020 | `operate-auto-confirm` + `operate-migrate` (CLI-023) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`) |
| CLI-021 | `operate-review`, `operate-run-web-form` | `rust/cli021-misc` | **merged** — #1022 (`c511736b`) |
| CLI-022 | `operate-mcp` | `rust/cli022-mcp` | **merged** — #1023 (`4c0236fb`) |
| CLI-023 | `operate-migrate` (validateRoots scope) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`); engine behind validation stays fail-closed, see Known defects |
| CLI-024 | migration detection, report, backup/state | local `main` | **integrated locally** — macOS and focused native Linux pass; native Windows and exact-head CI remain open |
| CLI-025 | `operate-poll-inbox` | local `main` | **integrated locally** — real IMAP adapter, source-bound CLI replay; corpus now 175 selected/0 deferred after LLM transport integration |

## Historical continuation (2026-09-22, CLI-024)

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

## Historical deferred-case queue

| Case id | Subsystem | State |
|---|---|---|


| `operate-generate-dashboard/-report/-scheduler` | generators | done — #1021 |
| `operate-generate-rebuttal`, `operate-classify-reply` | LLM | integrated locally — real Rust llmkit chat transport and exact missing-key Go CLI replay (`e8a8c96`) |
| `operate-migrate` | migration engine | implemented — #1024, validateRoots scope only (engine fail-closed, see Known defects) |
| `operate-review`, `operate-run-web-form` | misc | **merged** — #1022 (selected 160/6) |
| `operate-mcp` | MCP stdio server | **merged** — #1023 (`4c0236fb`) |
| `operate-poll-inbox` | CLI adapter | integrated locally — real CLI dispatch and transport replay; native trust-store behavior remains a separate gate |
| `operate-auto-confirm`, `operate-migrate` | triage / migration | **merged** — #1024 (`7d02cc58`) |

## CLI-025 execution notes

The prior "unported IMAP transport" blocker was disproved by source and
history. `crates/symeraseme-cli/src/mcp/handler.rs::poll_inbox` constructs the real
production dialer, with the source-bound nine-case handler fixture and eleven-case
transport corpus. CLI-025 now calls that handler.
`cmd/symeraseme/extra_commands.go:92-164` supplies the exact CLI contract:
only explicitly changed flags enter the argument map, `--since` and `--since-days`
share one value (last spelling wins), text output prefers a nonempty `message`,
otherwise prints `success`. Preserve these differences from other thin wrappers.
The existing `operate-poll-inbox` recording exercises a real refused local TCP
connection, not a transport-emulation string. Its native call and invalid
`--since` case now match Go; after the LLM transport slice the selected/deferred
counters are 175/0.

## CI caveat

`Rust / native (${{ matrix.os }})` reports **skipping** on PRs, so a green PR
is not native multi-platform evidence. The Rust push-to-main CI passed on
integrated `3f133e75`, including native OS jobs. This is not a cutover or
prerelease approval: the retained older Go rollback binary and release
artifact gates still require separate evidence.

## Pinned as measured, not desired

- **`plan execute --dry-run` on a web-form request appends a `SENT` event.**
  Go's `executeWebformRequest` gates only the adapter on `dry_run`, not the
  event append (the early return at `internal/campaign/execution.go:100`
  applies only when the runner is nil, and the CLI supplies one). It sends
  nothing, but it is not read-only, and the recorded `SENT` removes that
  request from the next batch. Changing it is a contract change (CLI-017).

## Remaining parity gaps

- **CLI-024 release acceptance.** The integrated native CI matrix passed on
  `3f133e75`, including the bounded registry build at 1024 descriptors
  (#1034). The disposable switchbacks build Go from current source; a
  retained older Go rollback binary reading schema v2 remains unproved
  (#1035). Do not promote this row to cutover-ready based on CI alone.
- **MCP malformed-stream breadth.** Ten source-bound Go malformed/adjacent/
  truncated process cases and ten parse/size/depth mutations now match Go
  1.26.6; broader bounded fuzz/performance evidence remains open under MCP-015.
- **`auto_confirm` with a stored reply (CLI-020).** Fails closed with an
  explicit message where Go runs `confirmation.AutoConfirm` (browser
  subsystem unported). The recorded case is the no-reply branch.
- **`go_map_order` exemption (CLI-020).** `ToolHandler::call` sorts every
  result except `auto_confirm` (Go structs keep declaration order). Any
  future Go-struct-returning tool needs the same exemption — grep the
  comment in `mcp/handler.rs`.

## Fixed parity defects (folded into the corpus or a regression test)

- **Workspace-guard edge strings.** The root-open and `InvalidInput` branches
  now match Go, with three focused Rust cases and two Go source-bound cases.
- **MCP HTTP transport and malformed-stream text.** Live HTTP starts with
  bearer auth/token rotation and signal drain; ten malformed stdio cases now
  replay Go process exit/stdout/stderr. Native platform evidence remains open.
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
