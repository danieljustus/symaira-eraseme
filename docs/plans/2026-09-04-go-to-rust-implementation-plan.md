# Symaira EraseMe Go → Rust implementation plan

> **For Hermes:** Execute this plan with `subagent-driven-development`. Headings
> are merge milestones; each checked step/TDD cycle is the bounded execution
> unit and gets a focused implementer plus spec and code-quality review. Never
> dispatch two writers against the same worktree or generated fixtures.

**Goal:** Replace the Go backend with an idiomatic Rust backend while preserving
all CLI, MCP, data, security, SwiftUI and release contracts, with a tested Go
fallback through the first stable Rust release.

**Architecture:** A three-crate internal Cargo workspace separates deterministic
core behavior, external adapters/orchestration and the executable surface. Go
stays runnable as the oracle. Every vertical slice is accepted through a
language-neutral black-box harness before integration.

**Tech stack:** Rust 1.98.0 stable, Cargo workspace, Clap, Serde, Rusqlite,
RustCrypto, Tokio/Axum/Reqwest-rustls, tracing, Nextest, llvm-cov, audit, deny,
proptest/fuzzing; existing Go 1.26.6 and Swift 5.10/Xcode gates remain active.

**Approval trigger:** Daniel's next reply `go` authorizes the full execution
workflow below: branches/worktrees, local commits, pushes, PRs, CI repair and
verified squash merges. It does not authorize contract changes, secret access,
real-email sends, live broker submissions or destructive testing on real data.

**Machine-readable DAG:**
`docs/plans/2026-09-04-go-to-rust-task-graph.json` owns task dependencies,
writer scopes and merge gates. This document owns task details; contradictions
block execution until both are reconciled.

---

## Execution protocol

### Coordinator duties

- Re-read `AGENTS.md`, this plan, the proposal and contract matrix.
- Verify `main`, origin, Git identity and a clean tree before each worktree.
- Create worktrees under `.worktrees/rust-<task>`; workers never create or
  remove worktrees and never work on `main`.
- Run Antigravity quota preflight before substantial delegation. Use
  `Gemini 3.8 Flash` with effort matched to the task; on quota/auth/rate-limit
  failure switch once to native `delegate_task` without retry loops.
- Give each worker the complete task text, exact worktree path, acceptance
  commands and prohibition on push/PR creation.
- Treat one checkbox or one inseparable test→implementation→verification cycle
  as the normal worker unit; do not hand a whole multi-day milestone to one
  agent.
- Before dispatch, write a bounded context packet to
  `docs/rust-port/packets/<task>-<base-sha>.md` containing only the task,
  relevant matrix rows, allowed files, exact commands, timeout, and recovery
  point. Default budgets: 20 minutes for implementation and 10 minutes per
  review; one fresh fallback worker after a timeout, then escalation.
- After implementation, dispatch a fresh **spec reviewer**. Fix all gaps and
  repeat until PASS, capped at three revision cycles.
- Then dispatch a fresh **quality/security reviewer**. Fix all critical and
  important issues and repeat until APPROVED, also capped at three cycles.
- Persist each reviewer verdict at
  `docs/rust-port/reviews/<task>-<reviewed-sha>-{spec,quality}.md` with reviewed
  SHA, commands/results, findings, verdict, and reviewer lane. A chat summary
  alone is not durable evidence.
- Escalate if a three-cycle review fails or the issue count does not decrease
  between consecutive reviews; never loop indefinitely.
- Re-run the coordinator-owned gates, inspect the diff, commit if needed, push,
  create the PR, verify CI and squash-merge only when green.
- Re-sync `main`, remove the worktree only after checking dirty state,
  unpushed commits and running processes.
- Update checkboxes, matrix statuses and the task graph's `execution_state` in
  the same PR as the slice; clear stale worktree entries after verified merge.
- At every phase boundary, checkpoint merged state and start the next phase
  with fresh orchestration context rather than carrying the full migration
  transcript forward.
- On worker timeout or partial output, preserve the worktree, record status and
  diff in the context packet, verify the partial changes, and dispatch a fresh
  recovery worker. Never reset or delete uncommitted partial work blindly.

### Invariants for every code task

1. Write or import the failing contract test first.
2. Run it and record the expected failure.
3. Implement the smallest vertical behavior.
4. Run the focused Rust test.
5. Run the matching Go oracle test.
6. Run the focused Go↔Rust differential case.
7. Run `cargo fmt --all --check` and Clippy with warnings denied.
8. Run the affected Go suite; Go must never become unrunnable.
9. Commit one coherent slice; no drive-by refactors.

### Canonical gates during dual-language development

```text
make go-gate
make rust-gate
make parity
make app-test
make release-dry-run
```

Those targets are introduced below. Until they exist, use the current commands
verbatim: `make fmt-check test lint vet build coverage`.

---

## Phase 0 — approval, blockers and frozen oracle

### Task 0.1: Accept architecture and open the migration epic

**Files:**
- Existing: `docs/plans/2026-09-04-go-to-rust-migration-proposal.md`
- Existing: `docs/plans/2026-09-04-go-to-rust-task-graph.json`
- Existing: `docs/rust-port-contract-matrix.md`
- Existing: this plan
- GitHub: one migration epic with linked stage issues

**Steps:**
- [ ] Treat the exact reply `go` as proposal acceptance.
- [ ] Re-check open issues/PRs and link #794–#800 as blockers.
- [ ] Put the four approved planning documents on an isolated docs branch,
      open a docs-only proposal PR, verify its diff, and squash-merge it before
      any implementation worktree branches from `main`.
- [ ] Create one migration epic and one issue per phase; copy acceptance gates,
      not the whole plan.
- [ ] Record the epic URLs in this document.
- [ ] Do not close the epic until `CUT-005` is decided.

**Verification:** `gh issue view <epic>` lists every phase and #794–#800.

**GitHub tracking:**

| Scope | Issue |
|---|---:|
| Migration epic | #813 |
| Phase 0 | #802 |
| Phase 1 | #803 |
| Phase 2 | #804 |
| Phase 3 | #805 |
| Phase 4 | #806 |
| Phase 5 | #807 |
| Phase 6 | #808 |
| Phase 7 | #809 |
| Phase 8 | #810 |
| Phase 9 | #811 |
| Phase 10 | #812 |

### Task 0.2: Freeze repository and toolchain baselines

**Files:**
- Create: `rust-tests/parity/baselines/v0.12.1.json`
- Create: `rust-tests/parity/README.md`
- Create: `scripts/capture-go-baseline.sh`

**Steps:**
- [x] Validate `v0.12.1` resolves to
      `240bf67cefa05e643e32611a02e6e7ed87a033ea`.
- [x] Capture source counts, exact Go coverage counts, binary hash/size,
      100-run startup distribution and RSS without personal data.
- [x] Capture the six released archive names/sizes, checksums format and DMG
      bundle manifest.
- [x] Record OS/architecture/tool versions with the measurements.
- [x] Make the capture script deterministic except explicitly timestamped
      metadata.

**Verification:** run the script twice; stable fields compare equal.

### Task 0.3: Resolve known blockers #794–#800 and #816–#817 before affected phases

**Files:**
- Modify: `scripts/package-dmg.sh`
- Modify: `.github/workflows/release.yml`
- Test/update: release packaging verification scripts
- Modify: `docs/event-store.md`
- Test/update: exact event-store encryption-header byte/length contract test
- Modify: `cmd/symeraseme/real_commands.go` and storage wiring required by #796
- Test/update: persistent DB path/encryption/restart contract tests
- Modify: `app/SymairaEraseMe/Sources/SymairaEraseMe/Services/MCPClient.swift`
- Test/update: exact Swift `tools/list` response parsing tests
- Test/update: Python-final/Go encryption interoperability fixtures required by
  #798
- Modify/test: production inbox adapter/HWM wiring required by #799
- Modify/test: production web-form boundary required by #800
- Modify/test: identity profile interoperability and decrypt-only key lookup
  required by #816
- Modify/test: constant-time MCP bearer comparison required by #817

**Steps:**
- [ ] Execute each issue in its own worktree/branch/PR; serialize overlapping
      storage/crypto changes and base each successor on freshly merged main.
- [ ] Implement the issue exactly as accepted, independently of Rust.
- [ ] Notarize and staple the signed app before creating the DMG so its offline
      ticket survives packaging.
- [ ] Sign the DMG container after creation and before notarization.
- [ ] Fail closed when required signing/notarization credentials or stapling
      fail.
- [ ] Verify nested binary, app bundle and DMG separately with `codesign`,
      `stapler validate` and the appropriate `spctl` assessment.
- [ ] Redownload the uploaded release asset and verify the published bytes,
      rather than trusting only the workspace copy.
- [ ] Run a packaging dry run and the issue-specific tests.
- [ ] Merge #794 before Phase 9 begins; rebase the migration work on it.
- [x] Resolve #795 before generating crypto vectors: correct the documented
      lengths to 17 bytes without changing header bytes, and pin the raw bytes
      plus lengths in a Go test.
- [x] Merge #795 before Phase 4 begins; regenerate oracle fixtures afterward.
- [ ] Resolve #796 before any oracle capture: production CLI/MCP storage must
      honor the documented default, `SYMERASEME_DB_DIR` and
      `SYMERASEME_ENCRYPT_DB`, including safe plaintext/encrypted upgrades.
- [ ] Prove restart durability and WAL/encryption cleanup in isolated dirs;
      merge #796, then generate the Go oracle fixtures.
- [x] Resolve #797 before app parity: parse the raw `tools/list` result separately
      from the `tools/call` content envelope and test the exact 26-tool shape.
- [x] Resolve #798 before crypto design: generate real standard-Fernet fixtures
      from `python-final`, make Go read them, and assign a distinct version/header
      to any incompatible Go AES-GCM format instead of reusing V1/V2/V3.
- [x] As part of #798, sweep `README.md`, `TROUBLESHOOTING.md`, `skills/**` and
      code comments for stale “Fernet = AES-GCM” claims; align every statement
      with the corrected versioned formats.
- [x] Resolve #799 with a real production IMAP adapter plus persistent HWM and
      an executable fake-server transcript; no real mailbox in tests.
- [ ] Resolve #800 by choosing and testing an honest runtime browser boundary
      or explicit manual fallback; missing `symbrowse` must never claim success.
- [x] Resolve #816 with real Python profile fixtures, frozen path/serialization/
      hash bytes, and read-only key lookup on every decrypt path.
- [x] Resolve #817 with strict Bearer parsing and constant-time secret
      comparison while preserving statuses and loopback defaults.

**Gate:** `DB-000 = PASS` before Task 0.4, `CRY-000` and `CRY-000B = PASS`
before Phase 4, `ID-000 = PASS` before identity/Rust fixture work,
`MCP-000 = PASS` before MCP oracle capture, `DOM-000A` and `DOM-000B = PASS`
before Tasks 6.3 and 7.2, and `APP-000` plus `REL-008 = PASS` before Phase 9.

### Task 0.4: Generate complete Go oracle fixtures

**Files:**
- Create: `scripts/generate-go-oracle-fixtures.sh`
- Create: `rust-tests/parity/cases/cli/*.json`
- Create: `rust-tests/parity/cases/mcp/*.jsonl`
- Create: `rust-tests/parity/cases/http/*.json`
- Create: `rust-tests/parity/cases/filesystem/*.json`
- Create: `rust-tests/parity/fixtures/README.md`

**Steps:**
- [ ] Enumerate the full Cobra command tree recursively, including hidden
      aliases, positional forms and every flag/default.
- [ ] Capture help, success, parse failure, missing argument and unknown flag
      behavior with raw stdout/stderr and exit status.
- [ ] Add raw MCP initialize/list/call/batch/notification/error cases.
- [ ] Add HTTP method/auth/origin/body-size cases.
- [ ] Use isolated HOME/XDG/TMPDIR, UTC, fixed locale and fake network servers.
- [ ] Never read the developer keychain, profile or real database.
- [ ] Mark every nondeterministic field explicitly; do not normalize broadly.

**Verification:** generator reproduces committed fixtures from the pinned Go
revision with no diff.

---

## Phase 1 — Rust foundation and black-box harness

### Task 1.1: Create the pinned Cargo workspace

**Files:**
- Create: `rust-toolchain.toml`
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `deny.toml`
- Create: `crates/symeraseme-core/Cargo.toml`
- Create: `crates/symeraseme-core/src/lib.rs`
- Create: `crates/symeraseme-engine/Cargo.toml`
- Create: `crates/symeraseme-engine/src/lib.rs`
- Create: `crates/symeraseme-cli/Cargo.toml`
- Create: `crates/symeraseme-cli/src/main.rs`
- Modify: `.gitignore`

**Steps:**
- [ ] Pin Rust `1.98.0` with rustfmt and Clippy components.
- [ ] Set workspace resolver, common package metadata, `rust-version = "1.98"`
      and release profile explicitly.
- [ ] Start every production crate with `#![deny(unsafe_code)]`.
- [ ] Add only dependencies needed for the empty executable and tests.
- [ ] Commit `Cargo.lock`.
- [ ] Build a Rust executable named `symeraseme-rust` during shadowing; reserve
      final output name `symeraseme` for cutover packaging.

**Verification:** `cargo fmt --all --check`, `cargo check --workspace
--all-targets`, Clippy and doctests pass.

### Task 1.2: Add the neutral differential harness

**Files:**
- Create: `rust-tests/parity/Cargo.toml`
- Create: `rust-tests/parity/src/main.rs`
- Create: `rust-tests/parity/src/{case,process,filesystem,sqlite,http,mcp,compare}.rs`
- Add to workspace: `rust-tests/parity`

**Steps:**
- [ ] Define data-driven cases containing argv, stdin bytes, env allowlist,
      cwd layout, timeout and expected comparison modes.
- [ ] Launch Go and Rust in separate process groups with fresh HOME/XDG trees.
- [ ] Capture raw status/signal/stdout/stderr before decoding.
- [ ] Hash recursive filesystem manifests including type and mode.
- [ ] Snapshot SQLite schema and ordered queries from copied databases.
- [ ] Record mock HTTP exchanges and raw MCP frames.
- [ ] Kill the process group on timeout.
- [ ] Implement only narrow, reason-tagged normalizers.

**Verification:** a deliberately different dummy Rust output fails with an
actionable field-level diff; identical fixture programs pass.

### Task 1.3: Add dual-language Make targets

**Files:**
- Modify: `Makefile`

**Steps:**
- [ ] Preserve current `build`, `test`, `coverage`, `lint` semantics for Go
      until cutover.
- [ ] Add `build-go`, `go-gate`, `build-rust`, `rust-gate`, `parity`,
      `app-test`, `release-dry-run`.
- [ ] Keep output paths separate and prevent stale binary reuse.
- [ ] Make `clean` remove generated Rust/Go test artifacts without touching
      committed fixtures.

**Verification:** current Go commands still pass exactly; new Rust targets pass.

### Task 1.4: Add shadow Rust CI

**Files:**
- Create: `.github/workflows/rust-ci.yml`
- Modify: `.github/dependabot.yml`

**Steps:**
- [ ] Add Ubuntu PR gate: fmt, check, Clippy, Nextest, doctests and parity subset.
- [ ] Enforce Rust line coverage at 80% overall and 90% for first-party crypto,
      consent/auth and MCP protocol modules; report exact covered/total counts.
- [ ] Add comprehensive main/schedule native matrix for macOS, Linux and
      Windows.
- [ ] Add `cargo audit`, `cargo deny check` and feature checks.
- [ ] Cache Cargo safely using lockfile keys.
- [ ] Keep Go CI required and unchanged.
- [ ] Do not add path filters that make a required PR check disappear.
- [ ] Read back the current ruleset contexts (`lint`, `test (3.12)`,
      `schema-validate`, `secrets-scan`). Add a stable Rust aggregate gate only
      after that check has run successfully on the default branch; never point
      protection at a check name that has not existed yet.

**Verification:** docs-only behavior and all required-check names are consistent
with repository rulesets; a PR runs both Go and Rust fast gates.

---

## Phase 2 — surface skeleton and deterministic foundations

### Task 2.1: Port version and build metadata

**Files:**
- Create: `crates/symeraseme-core/src/version.rs`
- Modify: `crates/symeraseme-cli/src/main.rs`
- Test: `crates/symeraseme-cli/tests/version.rs`

**Steps:**
- [ ] Add failing tests for text, root `--version` and JSON schema v1.
- [ ] Inject version through Cargo build environment without timestamps.
- [ ] Match trailing newlines and key order exactly.
- [ ] Pass `CLI-002..004` and the Swift `version --json` handshake expectation.

### Task 2.2: Port time and confirmation pure functions

**Files:**
- Create: `crates/symeraseme-core/src/timeutil.rs`
- Create: `crates/symeraseme-core/src/confirmation.rs`
- Test: matching module tests plus parity cases

**Steps:**
- [ ] Port all accepted timestamp layouts and UTC formatting.
- [ ] Port link extraction/scoring and negative cases.
- [ ] Add proptest coverage for timestamp round trips and hostile URLs.
- [ ] Compare against Go fixtures, not rewritten expectations.

### Task 2.3: Port configuration precedence

**Files:**
- Create: `crates/symeraseme-core/src/config.rs`
- Test: `crates/symeraseme-core/tests/config_parity.rs`

**Steps:**
- [ ] Reproduce defaults/global/project/env order and exact paths.
- [ ] Cover all supported `SYMERASEME_*` names and boolean parsing.
- [ ] Test missing home, malformed TOML, unknown fields and relative paths.
- [ ] Keep tests inside isolated HOME/CWD.
- [ ] Pass `CFG-001..006` and `CLI-009`.

### Task 2.4: Scaffold the complete Clap command tree

**Files:**
- Create: `crates/symeraseme-cli/src/cli.rs`
- Create: `crates/symeraseme-cli/src/commands/mod.rs`
- Create one command module per top-level command group
- Test: `crates/symeraseme-cli/tests/command_surface.rs`

**Steps:**
- [ ] Define every visible command, hidden alias, positional and flag/default.
- [ ] Snapshot help and parser errors from the oracle corpus.
- [ ] Route unported handlers to an internal test-only boundary; never ship a
      fake success response.
- [ ] Match Cobra output deliberately where Clap defaults differ.
- [ ] Pass `CLI-001`, `CLI-005..008` parser/help portions.

---

## Phase 3 — registry, templates and redaction

### Task 3.1: Port registry models and strict validation

**Files:**
- Create: `crates/symeraseme-core/src/registry/{mod,model,validate}.rs`
- Test: `crates/symeraseme-core/tests/registry_contract.rs`

**Steps:**
- [ ] Model every schema-v1 field with exact snake_case names/defaults.
- [ ] Reject unknown keys at every closed object boundary.
- [ ] Implement channel variant, enum, selector, URI/date and filename rules.
- [ ] Run all four golden and negative fixtures.
- [ ] Run the full real corpus; expected count is 1,277.

### Task 3.2: Port registry embedding, loading, filtering and sync

**Files:**
- Create: `crates/symeraseme-core/src/registry/{loader,filter,sync}.rs`
- Create: `crates/symeraseme-core/build.rs` if build-time generation is chosen
- Test: loader/filter/sync parity tests

**Steps:**
- [ ] Embed committed registry data without runtime filesystem dependency.
- [ ] Preserve underscore skipping, deterministic ordering and first/all-error
      behavior.
- [ ] Preserve status/disabled filter semantics.
- [ ] Mock HTTPS sync and verify safe validated replacement.
- [ ] Measure lazy/eager loading; do not add a binary cache unless measured and
      contract-neutral.
- [ ] Pass `REG-001..008`.

### Task 3.3: Port legal and report templates

**Files:**
- Create: `crates/symeraseme-core/src/templating.rs`
- Create only if needed: `crates/symeraseme-core/templates/`
- Test: `crates/symeraseme-core/tests/template_contract.rs`

**Steps:**
- [ ] First test MiniJinja against the existing canonical `.j2` sources.
- [ ] Accept it only if all 11 outputs equal `golden-templates.json` byte for
      byte.
- [ ] If not, port frozen templates into the Rust crate and document why.
- [ ] Do not change legal wording, whitespace or escaping to suit the engine.
- [ ] Pass `TMP-001..002`.

### Task 3.4: Port redaction

**Files:**
- Create: `crates/symeraseme-core/src/redaction/{mod,pii,review,path}.rs`
- Test: shared redaction corpus and property tests

**Steps:**
- [ ] Port every PII detector and profile-aware literal replacement.
- [ ] Preserve replacement ordering, overlap and UTF-8 behavior.
- [ ] Port safe path and consent-bound file review.
- [ ] Fuzz text input; assert no panic and no matched sentinel leaks.
- [ ] Pass `RED-001..002`.

---

## Phase 4 — SQLite, encryption, identity and consent

### Task 4.1: Prove Rusqlite portability before domain storage work

**Files:**
- Create: `crates/symeraseme-core/src/storage/mod.rs`
- Create: `.github/workflows/rust-target-proof.yml` or fold into Rust CI

**Steps:**
- [ ] Pin a maintained Rusqlite/libsqlite3-sys combination.
- [ ] Build and execute a WAL/transaction smoke test natively on all six
      supported OS/architecture targets where runners exist.
- [ ] Inspect linkage and document runtime requirements.
- [ ] Abort/escalate if current release portability cannot be matched.

### Task 4.2: Port schema and typed repository queries

**Files:**
- Create: `crates/symeraseme-core/src/storage/{store,repository,types}.rs`
- Test: `crates/symeraseme-core/tests/sqlite_contract.rs`

**Steps:**
- [ ] Reproduce schema v1 SQL, indexes, defaults and user_version behavior.
- [ ] Reproduce pragmas per connection.
- [ ] Read a copied `golden-campaign.db` without modifying the fixture.
- [ ] Match NULL/string/integer conversions and query ordering.
- [ ] Test read-only/newer/corrupt/locked databases.
- [ ] Pass `DB-001..003`, `DB-006`, `DB-009..010`.

### Task 4.3: Port event append and projection

**Files:**
- Create: `crates/symeraseme-core/src/storage/projection.rs`
- Test: projection and transaction parity tests

**Steps:**
- [ ] Port closed event/source validation.
- [ ] Replay by `occurred_at ASC, id ASC`.
- [ ] Preserve skip-vs-reject behavior for malformed historical rows.
- [ ] Write append+projection atomically and test forced rollback.
- [ ] Match all four event-store golden JSON files.
- [ ] Pass `DB-004..005`, `DB-007..008`.

### Task 4.4: Port the corrected, versioned encryption compatibility contract

**Files:**
- Create: `crates/symeraseme-core/src/storage/encryption.rs`
- Create: `rust-tests/parity/fixtures/crypto/`
- Test: bidirectional vectors and corruption corpus

**Steps:**
- [ ] Add deterministic vectors from `python-final` and the corrected Go oracle
      with injected clock/RNG.
- [ ] Implement the #798 result exactly: standard Fernet for shipped Python
      bytes and a distinct version only where incompatible Go AES-GCM bytes
      must remain readable. Never infer format from a reused ambiguous header.
- [ ] Prove Rust decrypts every shipped Python/Go variant.
- [ ] Prove the corrected Go oracle decrypts the Rust write format required for
      rollback.
- [ ] Test tamper, truncation, wrong key and unsupported header cases.
- [ ] Fuzz the envelope parser.
- [ ] Pass `CRY-001..006`.

### Task 4.5: Port encrypted temp lifecycle and locking

**Files:**
- Create: `crates/symeraseme-core/src/storage/{encrypted_store,locking}.rs`
- Test: native process/crash tests

**Steps:**
- [ ] Use secure per-user temp roots and 0700/0600 permissions.
- [ ] Lock per original database path.
- [ ] Checkpoint WAL before close/re-encryption.
- [ ] Write replacement atomically, fsync as required and preserve rollback.
- [ ] Test interruption at each mutation boundary with copied fixtures.
- [ ] Verify cleanup on success/error/signal.
- [ ] Pass `CRY-007..008`.

### Task 4.6: Port identity profile and key resolution

**Files:**
- Create: `crates/symeraseme-core/src/identity/{mod,profile,secrets,keyring}.rs`
- Test: cross-language profile/key fixtures

**Steps:**
- [ ] Reproduce encrypted profile bytes/AAD/schema and paths.
- [ ] Reproduce the master-key order exactly: in-process cache, direct hex env,
      passphrase-derived key, then OS keyring.
- [ ] Separately reproduce application-secret resolution for literals,
      `env://`, `keychain://`, canonical `symvault://`, legacy `vault://`, env
      fallback and keyring fallback.
- [ ] Use fake keyring/symvault adapters in normal tests.
- [ ] Run native keyring integration tests separately without printing values.
- [ ] Scan logs/errors for sentinel secrets.
- [ ] Pass `ID-001..003`.

### Task 4.7: Port consent tokens and destructive-operation gate

**Files:**
- Create: `crates/symeraseme-core/src/identity/{consent,gate}.rs`
- Test: fixed-clock/RNG and filesystem-mode cases

**Steps:**
- [ ] Match token generation, file hash/name, JSON, TTL and command matching.
- [ ] Preserve legacy path migration and revoke/list semantics.
- [ ] Make writes atomic and permission-safe across platforms.
- [ ] Prove MCP remains non-interactive and cannot bypass consent.
- [ ] Pass `ID-004..005`, `CLI-017`.

---

## Phase 5 — deterministic domain services

### Task 5.1: Port deadlines and tick engine

**Files:**
- Create: `crates/symeraseme-core/src/deadlines.rs`
- Test: shared tick golden and fixed-clock edge cases

**Steps:**
- [ ] Port transition/action timing, reminders, escalation and dry-run.
- [ ] Run fixed-clock edge cases and the shared tick golden.
- [ ] Pass `DOM-001` and `CLI-013` tick behavior.

### Task 5.2: Port manual tasks

**Files:**
- Create: `crates/symeraseme-core/src/manual_tasks/{mod,repository,service}.rs`
- Test: DB/artifact lifecycle tests

**Steps:**
- [ ] Preserve reason/status enums and completion notes.
- [ ] Preserve evidence file creation, retention and cleanup rules.
- [ ] Pass `DOM-010` and `CLI-020`.

### Task 5.3: Port reporting and read models

**Files:**
- Create: `crates/symeraseme-core/src/reporting/{mod,dashboard,report,calendar}.rs`
- Test: shared reporting fixture and output files

**Steps:**
- [ ] Preserve ordering, nullable fields and aggregation semantics.
- [ ] Preserve JSON/CSV/HTML bytes and output paths.
- [ ] Pass `DOM-003` and `CLI-018..019`.

### Task 5.4: Port scheduler generation and installation boundary

**Files:**
- Create: `crates/symeraseme-engine/src/scheduler/{mod,cron,launchd,systemd}.rs`
- Test: native snapshots with fake command runner

**Steps:**
- [ ] Preserve generated bytes, labels/paths, quoting and modes.
- [ ] Preserve dry-run and install/uninstall/status command behavior.
- [ ] Pass `DOM-009` and `CLI-014` on native platforms.

---

## Phase 6 — email, OAuth and LLM adapters

### Task 6.1: Freeze protocol transcripts and select maintained email crates

**Files:**
- Create: `rust-tests/parity/fixtures/email/`
- Create: `docs/rust-email-crate-decision.md`

**Steps:**
- [ ] Capture deterministic MIME, SMTP and IMAP fake-server transcripts.
- [ ] Evaluate current maintained crates against STARTTLS, XOAUTH2, cancellation,
      UIDVALIDITY/HWM and static build requirements.
- [ ] Choose crates only after transcript and native-target proof.
- [ ] Record rejected alternatives and locked versions.

### Task 6.2: Port MIME and SMTP

**Files:**
- Create: `crates/symeraseme-engine/src/email/{mod,mime,smtp}.rs`
- Test: raw byte and fake SMTP server cases

**Steps:**
- [ ] Preserve CRLF, header sanitization/order, message ID and boundary hash.
- [ ] Preserve recipient/BCC behavior, TLS minimum and auth.
- [ ] Verify failures redact credentials and tokens.
- [ ] Pass `DOM-005`.

### Task 6.3: Port IMAP parser and HWM state machine

**Files:**
- Create: `crates/symeraseme-engine/src/email/imap.rs`
- Test: fake IMAP transcript/state-machine tests

**Steps:**
- [ ] Use the corrected #799 Go production contract as oracle, not the former
      nil-dialer failure.
- [ ] Preserve UID range/order/truncation and UIDVALIDITY cold start.
- [ ] Preserve fetch-failure HWM advancement.
- [ ] Preserve header decoding, subject normalization and body bounds.
- [ ] Pass `DOM-006`.

### Task 6.4: Port OAuth2

**Files:**
- Create: `crates/symeraseme-engine/src/email/oauth2.rs`
- Test: mock HTTP/atomic state-file tests

**Steps:**
- [ ] Preserve PKCE, state validation and token refresh.
- [ ] Preserve timeouts and atomic state files.
- [ ] Strictly redact provider error bodies and credentials.
- [ ] Pass `DOM-007`.

### Task 6.5: Port LLM descriptor and transport layer

**Files:**
- Create: `crates/symeraseme-engine/src/llm/{mod,descriptor,client,agent}.rs`
- Create: generated provider contract under
  `crates/symeraseme-engine/contracts/`
- Create: `scripts/sync-llm-contract.sh`
- Test: provider mock-server transcripts

**Steps:**
- [ ] Generate descriptors from the exact pinned corekit contract with source
      tag/SHA metadata and drift verification.
- [ ] Reproduce Anthropic/OpenAI/Ollama/custom dialects, defaults, auth,
      timeouts, retries and error taxonomy.
- [ ] Port the host-agent subprocess boundary.
- [ ] Preserve current zero-usage behavior where corekit exposes none.
- [ ] Never log prompts, credentials or provider error bodies unexpectedly.
- [ ] Pass `DOM-008`.

---

## Phase 7 — application orchestration and legacy migration

### Task 7.1: Port campaign planning and batch execution

**Files:**
- Create: `crates/symeraseme-engine/src/campaign/{mod,planning,batch,execution}.rs`
- Test: `golden-plan.json` and forced-failure transactions

**Steps:**
- [ ] Preserve broker selection/order and identity snapshot hashing.
- [ ] Preserve dry-run, batch/concurrency semantics and partial failures.
- [ ] Preserve atomic event writes and state transitions.
- [ ] Pass `DOM-002` and `CLI-010`.

### Task 7.2: Port current web-form behavior without feature expansion

**Files:**
- Create: `crates/symeraseme-engine/src/campaign/web_form.rs`
- Test: existing HTML/fake-driver/manual-fallback cases

**Steps:**
- [ ] Use the corrected #800 Go production contract as oracle, not the former
      nil-driver failure.
- [ ] Port declarative conversion and result/reason/evidence mapping.
- [ ] Preserve the accepted runtime-integration or manual-fallback behavior.
- [ ] Do not introduce behavior beyond the settled #800 contract.
- [ ] Pass `CLI-023`.

### Task 7.3: Port triage, replies and confirmation orchestration

**Files:**
- Create: `crates/symeraseme-engine/src/{triage,replies}.rs`
- Test: broker reply corpus and mocked adapters

**Steps:**
- [ ] Preserve message/request correlation and classification categories.
- [ ] Preserve prompt-injection handling, confidence thresholds and save flags.
- [ ] Preserve event writes, confirmation and rebuttal flows.
- [ ] Pass `DOM-004` and `CLI-022`.

### Task 7.4: Port the Python-era migration command unchanged

**Files:**
- Create: `crates/symeraseme-engine/src/migration/{mod,inspect,run,verify,rollback}.rs`
- Create: `crates/symeraseme-cli/src/commands/migrate.rs`
- Test: copied migration fixtures only

**Steps:**
- [ ] Preserve non-destructive source handling, backup and resumable state.
- [ ] Preserve secret metadata-only behavior, verification and rollback.
- [ ] Never use a real home installation.
- [ ] Pass `CLI-024`.

---

## Phase 8 — MCP and complete CLI parity

### Task 8.1: Port the transport-independent MCP protocol core

**Files:**
- Create: `crates/symeraseme-cli/src/mcp/{mod,protocol,sanitize}.rs`
- Test: raw JSON-RPC corpus + property/fuzz tests

**Steps:**
- [ ] Preserve request IDs, notifications, batches, params and aliases.
- [ ] Preserve error codes/messages, content envelope and sanitization.
- [ ] Pin the initialize response and 26-tool JSON exactly.
- [ ] Run the official MCP conformance suite; use `rmcp` only where it does not
      change the pinned EraseMe wire behavior.
- [ ] Pass `MCP-001..008`.

### Task 8.2: Port stdio transport

**Files:**
- Create: `crates/symeraseme-cli/src/mcp/stdio.rs`
- Fuzz: `fuzz/fuzz_targets/mcp_stdio.rs`

**Steps:**
- [ ] Consume sequential JSON values/newline frames as the Go decoder does.
- [ ] Write protocol only to stdout and route tracing to stderr.
- [ ] Handle EOF, malformed input and cancellation.
- [ ] Pass `MCP-014..015`.

### Task 8.3: Port HTTP transport and lifecycle

**Files:**
- Create: `crates/symeraseme-cli/src/mcp/http.rs`
- Test: differential HTTP/process/signal tests

**Steps:**
- [ ] Preserve POST-only behavior, content type/statuses and the 5 MiB limit.
- [ ] Preserve bearer auth, Origin and loopback/remote bind policy.
- [ ] Preserve token rotation/mode and graceful shutdown.
- [ ] Pass `MCP-009..013`.

### Task 8.4: Wire all 26 handlers and complete CLI behavior

**Files:**
- Create/modify: command modules under `crates/symeraseme-cli/src/commands/`
- Create: `crates/symeraseme-cli/src/dispatch.rs`
- Test: full CLI/MCP fixture loop

**Steps:**
- [ ] Route CLI and MCP through one typed application dispatch layer.
- [ ] Implement every alias/default/output envelope.
- [ ] Remove all test-only “unported” handlers.
- [ ] Run every CLI and MCP case.
- [ ] Pass `CLI-001..024`, `MCP-001..015`.

### Task 8.5: Hardening sweep

**Files:**
- Create/update: `fuzz/`, property tests, security docs
- Create: `crates/symeraseme-core/tests/miri_contract.rs`
- Create fuzz targets: `mcp_frames`, `registry_yaml`, `encryption_envelope`,
  `path_inputs`, `email_headers`

**Steps:**
- [ ] Fuzz MCP frames, registry documents, encrypted envelopes, URLs/paths and
      email headers.
- [ ] Run Miri on suitable pure/core tests.
- [ ] Run mutation testing on crypto validation, consent, MCP auth/sanitization
      and state transitions.
- [ ] Test timeouts, signals, locked DBs, partial writes, read-only FS and
      interrupted upgrades.
- [ ] Run secret sentinel scans over stdout/stderr/generated files.

---

## Phase 9 — SwiftUI and release integration

### Task 9.1: Run SwiftUI tests against the Rust backend

**Files:**
- Modify only if build plumbing requires it:
  `app/SymairaEraseMe/build.sh`
- Test: `app/SymairaEraseMe/Tests/SymairaEraseMeTests/`

**Steps:**
- [ ] Place Rust `symeraseme` next to the Swift debug executable.
- [ ] Test binary discovery, startup, token read, tools/list, representative
      calls, error display and shutdown.
- [ ] Do not change Swift protocol code to accommodate a Rust mismatch.
- [ ] Pass `APP-001..004`.

### Task 9.2: Replace release build plumbing while preserving assets

**Files:**
- Modify: `scripts/package-dmg.sh`
- Modify: `.github/workflows/release.yml`
- Replace/retire after shadow proof: `.goreleaser.yml`
- Create: `scripts/package-rust-release.sh`
- Modify: `.github/workflows/publish-homebrew.yml`

**Steps:**
- [ ] Produce the same six archive names and archive-root layout.
- [ ] Evaluate `cargo-dist` against those exact artifacts; adopt it only if it
      replaces rather than duplicates the release pipeline and produces no
      contract drift. Otherwise keep the explicit packaging script.
- [ ] Include LICENSE and README exactly as before.
- [ ] Generate compatible SHA-256 `checksums.txt`.
- [ ] Build/test natively on supported OS targets; use cross-build only for
      additional confidence.
- [ ] Build the DMG with the Rust backend at
      `Contents/MacOS/symeraseme`.
- [ ] Apply nested binary, app bundle and DMG signatures in correct order;
      notarize and staple.
- [ ] Generate SBOMs and dependency metadata without changing existing asset
      names unexpectedly.
- [ ] Pass `REL-001..010` in a dry-run release.

### Task 9.3: Verify Homebrew and release consumers

**Files:**
- Separate repo after immutable prerelease assets exist:
  `/Users/daniel/Dev/Symaira Dev/homebrew-tap/Formula/symeraseme.rb`

**Steps:**
- [ ] Update only from real published asset checksums.
- [ ] Run Ruby syntax, style, strict audit and formula install/test on macOS;
      run Linux formula test in CI.
- [ ] Verify `symeraseme version` and `version --json` after install.
- [ ] Preserve formula token and binary name.
- [ ] Do not merge tap changes before the upstream prerelease exists.

---

## Phase 10 — reversible cutover and retirement

### Task 10.1: Ship a dual-backend prerelease

**Files:**
- Create: Rust fallback dispatch in CLI entrypoint
- Modify: `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `TROUBLESHOOTING.md`,
  release packaging and agent-skill wording that currently promises a Go
  backend

**Steps:**
- [ ] Package Rust as `symeraseme` and last-known-good Go as `symeraseme-go`.
- [ ] Implement explicit `SYMERASEME_BACKEND=go`: Unix process replacement;
      Windows spawn/wait with inherited stdio and exact exit propagation. Test
      argument, env and cwd preservation plus platform-appropriate signals.
- [ ] Test fallback absence and version mismatch loudly.
- [ ] Publish a prerelease; never overwrite stable assets.
- [ ] Document the dual-backend prerelease honestly: Rust is primary, Go is an
      explicit rollback backend, and historical Go-port documents stay marked
      as history rather than being silently rewritten.
- [ ] Run clean-install, upgrade, encrypted/plain DB and rollback smoke tests on
      macOS, Linux and Windows.
- [ ] Pass `CUT-001..003`.

### Task 10.2: Canary and stable Rust release

**Steps:**
- [ ] Dogfood copied/sanitized data only; never mutate the sole real dataset.
- [ ] Run the SwiftUI app against the prerelease backend.
- [ ] Verify GitHub artifacts, checksums, signatures, notarization, SBOMs and
      Homebrew installation from real URLs.
- [ ] Classify every observed delta; no unexplained parity mismatch survives.
- [ ] Publish stable only when every required matrix row is PASS.
- [ ] Record final binary size/startup/RSS and honest regressions/gains.
- [ ] Enforce the proposal's value gate: no unexplained >20% regression and a
      15% p95-startup or RSS gain, otherwise stop for an explicit exception.
- [ ] Keep `symeraseme-go` and rollback docs through this stable release.
- [ ] Schedule a durable release+7-day follow-up that rechecks open defects,
      release assets and rollback evidence before Task 10.4 starts.
- [ ] Pass `CUT-004`.

### Task 10.3: Final integration review

**Subagents:**
- [ ] Contract/spec reviewer: every matrix row and plan acceptance criterion.
- [ ] Rust quality reviewer: architecture, error handling, unsafe inventory,
      dependency and platform risk.
- [ ] Security reviewer: crypto, keyring, consent, auth, path handling,
      redaction and release provenance.
- [ ] Release reviewer: archives, Homebrew, DMG, signing/notarization, rollback.

**Coordinator verification:**

```text
make go-gate
make rust-gate
make parity
make app-test
make release-dry-run
cargo +nightly miri test -p symeraseme-core --test miri_contract
for target in mcp_frames registry_yaml encryption_envelope path_inputs email_headers; do
  cargo +nightly fuzz run "$target" -- -max_total_time=60
done
```

All GitHub required checks must be green on the final PR. Verify merged state by
reading the PR and main branch back from GitHub.

### Task 10.4: Retire Go in a separate reviewed change

**Prerequisite:** one stable Rust release has operated for at least seven days
without unexplained parity defects. The initial `go` already authorizes this
separate retirement PR once that gate is proven; no avoidable second approval
pause is introduced.

**Files:**
- Create: `docs/go-test-rust-port-classification.md`
- Remove only after classification: Go source, modules and Go-only workflows

**Steps:**
- [ ] Tag/preserve the final Go source and downloadable artifacts.
- [ ] Classify every Go test file as ported, contract-replaced or intentionally
      retired, with exact Rust test/fixture evidence; no unclassified test may
      disappear.
- [ ] Verify rollback from Rust-created plain and encrypted data once more.
- [ ] Remove Go source, `go.mod`, `go.sum`, Go CI and GoReleaser only in this PR.
- [ ] Update branch protection in safe order: prove the Rust aggregate check,
      require it, remove Go-only required contexts, read protection back, and
      only then delete workflows that emitted the old contexts.
- [ ] Remove fallback binary/env path and stale Go wording.
- [ ] Keep historical contract docs and migration records.
- [ ] Re-run every Rust/app/release gate.
- [ ] Update `AGENTS.md`, README, CONTRIBUTING, TROUBLESHOOTING and ecosystem
      inventory from Go to Rust.
- [ ] Pass `CUT-005`; close the migration epic.

---

## Final definition of done

- [ ] Every required row in `docs/rust-port-contract-matrix.md` is PASS.
- [ ] Go and Rust match on deterministic success, error and side-effect cases.
- [ ] Existing plain/encrypted databases and identity profiles work both ways
      through the rollback window.
- [ ] `cargo fmt`, check, Clippy `-D warnings`, Nextest, doctests, feature checks,
      llvm-cov, audit and deny pass.
- [ ] Rust coverage is at least 80% overall and 90% in the critical
      crypto/consent-auth/MCP modules.
- [ ] Parser/protocol fuzzing and suitable Miri tests pass.
- [ ] Native macOS/Linux/Windows tests pass for supported targets.
- [ ] SwiftUI integration passes without protocol concessions.
- [ ] Release assets, archive names/layout, checksums, signatures, notarization,
      SBOMs and Homebrew behavior are verified from real artifacts.
- [ ] Final performance measurements are published against the Go baseline;
      regressions are explicit, not massaged away.
- [ ] Rollback is documented and exercised before Go retirement.
- [ ] Repository is clean, `main` is synchronized, worktrees are removed and
      the final merged/released state is read back and verified.
