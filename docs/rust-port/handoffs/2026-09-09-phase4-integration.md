# Phase 4 integration handoff — 2026-09-09

## Scope and decision

This handoff records the verified state of the **existing Go → Rust migration**.
It does not authorize a product, repository, package, or module move.  The
Go implementation remains the production reference and must not be removed.
The applicable product-boundary decision is `PB-2026-09-09` revision 2 from
the sibling `docs` repository: Browse, Operate, and Scope are optional Brain
modules; Cockpit is hardware/system tuning only.  That decision is an input to
future consolidation planning, not a change to this repository.

The sibling docs contract was read from local branch
`docs/product-boundaries-20260909`; it was **not asserted to be on this
repository's `main`**.  Its commit mapping remains in
`../docs/intern/product-boundaries-20260909/commits.json`.

## Verified integration baseline

| Area | Integration commit / PR | Affected artifacts | Evidence |
|---|---|---|---|
| Phase 3 registry, templates, redaction | `fa366dca` / #870 | registry, templates, redaction Rust modules and parity fixtures | Integrated before this handoff; Go oracle remains pinned by the matrix. |
| ID-003 secret-resolution redaction | `9d67b77f` / #895 | `identity/resolve.rs`, `identity_secret_resolution.rs` | Sentinel-secret contract test is committed; no production cutover. |
| ID-004 consent token/gate slice | `d7591d8c` / #897 | `identity/consent.rs`, `identity/gate.rs`, `consent_api.rs` | Focused deterministic consent contract was run locally.  This is a slice only: task 4.7 remains open until ID-005 and CLI-017 pass. |
| Phase 4.1 bundled SQLite portability | `9c1509c48e023f04877d1ef9fe89cac4057326d6` / #898 | bundled `rusqlite`, storage configuration, native target workflow, `storage_portability.rs` | PR #898 was merged only after required checks and the Rust fast gate passed on its exact head. |
| Windows lint follow-up | `9df2e83cbf9c06eb2a768b3f4ffeb40c994f3506` / #899 | Unix-gated consent permissions and tests | Fixes the post-#898 Windows-native lint failure without changing the consent contract. |
| Windows debug-path test follow-up | `d4c5331543228832eb94faf85990a9111642165c` / #900 | `consent_api.rs` debug-path assertion | Compares `Path` debug output with the same representation emitted by `ConsentStore`, fixing the remaining Windows-only test failure. |

`origin/main` was read back at
`d4c5331543228832eb94faf85990a9111642165c` after #900 merged.  The exact
#898 PR fast-gate run was
[`34355503310`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34355503310)
and completed successfully.  Its completed jobs included format, clippy,
tests, line coverage, every feature combination, dependency audit/policy, and
the neutral parity subset.  The Phase 4.1 native workflow passed the bundled
SQLite smoke on macOS arm64/amd64, Linux arm64/amd64, and Windows arm64/amd64.

The first post-#898 `main` Rust CI run
[`34357031453`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34357031453)
failed solely because Windows compiles Unix-only permission variables and a
Unix-only test as unused under `-D warnings`.  #899 corrects those `cfg`
boundaries.  Its required checks and its Rust PR fast gate passed before
merge.  The post-#899 Windows run then exposed a second platform-specific
test assumption: `Path` Debug formatting escapes Windows separators whereas
the test used the unescaped display form.  #900 fixes that assertion and
passed required checks plus its Rust PR fast gate before merge.  The
post-#900 `main` CI was read back on the exact SHA above: Rust CI,
Rust SQLite target proof, CodeQL, and the primary CI all completed
successfully.  The corresponding runs are
[`34363503671`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34363503671),
[`34363503606`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34363503606),
[`34363503517`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34363503517),
and
[`34363503439`](https://github.com/danieljustus/symaira-eraseme/actions/runs/34363503439).

## Commands and observed results

The following commands were executed against isolated migration worktrees;
no production data directory or user database was used.

| Command | Platform | Result |
|---|---|---|
| `GOTOOLCHAIN=go1.26.6 make go-gate` | macOS arm64 | PASS (Go vet, build, tests; reported coverage 75.95%). |
| `dev-external cargo fmt --all --check` | macOS arm64 | PASS. |
| `dev-external cargo check --workspace --all-targets --all-features --locked` | macOS arm64 | PASS. |
| `dev-external cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | macOS arm64 | PASS. |
| `dev-external cargo test -p symeraseme-core --test storage_portability --locked` | macOS arm64 | PASS: `sqlite_portability_smoke_uses_isolated_file_wal_and_transactions` (1/1). |
| `dev-external cargo test -p symeraseme-core --test consent_api --locked` | macOS arm64 | PASS. |
| PR #898 required checks | GitHub CI | PASS: `lint`, `test (3.12)`, `schema-validate`, `secrets-scan`. |
| Rust fast gate run `34355503310` | GitHub CI | PASS; includes dependency and neutral-parity gates. |
| Native bundled-SQLite target smoke | GitHub native macOS/Linux/Windows arm64/amd64 | PASS on all six supported targets. |
| #899 required checks and Rust PR fast gate | GitHub CI | PASS before merge; fixes the Windows-only post-#898 lint regression. |
| #900 required checks and Rust PR fast gate | GitHub CI | PASS before merge; fixes the remaining Windows-only debug-path test assumption. |
| Post-#900 main CI | GitHub CI on `d4c53315` | PASS: Rust CI, Rust SQLite target proof, CodeQL, and primary CI. |

Cross-compilation is not treated as a native runtime result.  The target proof
above is native CI evidence; local macOS testing is additional evidence only.

## Contract status and remaining migration work

The current Rust work is a verified **partial migration**, not a completed
replacement.  The Go CLI and MCP server remain the live public contract and
reference implementation.  The matrix still has open rows for, among others:

- CLI-010 through CLI-024 (command behavior, flags, exit codes and stderr),
- DB-001 through DB-010 except the separate Phase 4.1 portability proof,
- CRY-001 through CRY-008 and ID-001/ID-002/ID-005,
- MCP-001 through MCP-015 except the existing Go-side HTTP hardening rows,
- domain, release, Swift integration, cutover and rollback rows.

Open dependent implementation branches were preserved, not rebased or merged
blindly: #882, #883, #886, #887, #888, #890, #891, #892, and #896.  The safe
serial continuation, after reading fresh `origin/main`, is: crypto #882 →
#891 → #892; storage #883 → #886 → #887 → #888 → #890.  #896 is superseded in
practice by merged #897 but remains an untouched remote draft; do not use it
as a source of truth without a fresh diff.  Issue #889's unknown-replay
semantics must be verified while integrating #890.

## Reference, rollback, and data safety

- **Reference path:** keep the pinned Go oracle commit
  `bf53346eec234929bedf0314b99e3da85dbb991b` reproducible, run the existing
  Go targets, and use `rust-tests/parity` for differential cases.
- **Rollback path:** production dispatch remains Go; no Rust package, release
  artifact, migration command, or backend cutover was enabled.  Revert a
  Rust-only integration commit if needed; do not mutate real stores.
- **Data ownership:** Go owns the current event-store SQLite schema, encrypted
  profile/data files, consent files, and operational data directories.  Rust
  test cases use isolated temporary copies/files only.
- **Public entry points:** `symeraseme` CLI and its `mcp` HTTP/stdio modes are
  currently Go-owned.  Rust crates are internal shadow implementation modules
  until the matrix's CLI/MCP/cutover rows pass.
- **Direct consumers / native dependencies:** the SwiftUI macOS app discovers
  and supervises the backend binary and uses token-authenticated MCP.  Native
  concerns still requiring contract work include keychain/SymVault access,
  filesystem permission modes, process signals, scheduler integration,
  codesigning/notarization, and the existing app helper lifecycle.

## Worktree and branch handoff

This documentation branch is `docs/rust-phase4-handoff-20260909`, based on
`9c1509c4`.  It contains only the ledger/matrix/handoff update and should be
reviewed and merged serially like every other migration PR.  Other existing
worktrees and remote branches are deliberately untouched.  Before another
integration, re-read `origin/main`, the PR head, required checks, and this
handoff; never infer completion from a stale branch or a compiling Rust crate.

## Verdict

**STABILER TEILSTAND, MIGRATION NOCH OFFEN.**

This repository is **not ready for a behavior-preserving module move**:
public CLI, MCP, lifecycle, encrypted-data, release, and rollback contracts
are still materially open.  That is separate from release approval and does
not authorize consolidation work here.
