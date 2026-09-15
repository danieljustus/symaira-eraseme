<!-- review: timestamp=2026-09-15T10:47:43Z  repo=danieljustus/symaira-eraseme  head=e21cc7c996deddcd9a1d3791c7db76bcb8352d76 -->

# Code Review — 2026-09-15

## Scope and verification
Reviewed the isolated clean worktree at `e21cc7c996deddcd9a1d3791c7db76bcb8352d76` (`migration/rust-integration-20260913`) against live `origin/main` `e2a4e5cd048603cc83205397f4320a83e8a1034a`, with profile-read source `68fb998ef2568c855cd579dc0438b9d266801d32`. The integration tree contains the crypto V1/V2/V3 and legacy-Go compatibility slice, SQLite lifecycle/shared-opening slice, and profile-read prerequisite. The pinned CoreKit SQLite source is declared at `Cargo.toml:34` and was compiled by Cargo at revision `62edd9903983d9369373565cc1e50da3fef43176`. No source, test, dependency, index, history, or remote state was changed; this report is the only intentional review artifact.

Local evidence, all run with private HOME/XDG/TMPDIR and a serialized heavy-slot lock:

- `cargo fmt --all --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features --locked` — PASS: 69 core unit tests, 1 parity unit test, 1 CLI unit test, and all integration suites; 1 consent filesystem test intentionally ignored.
- Focused `encryption_parity` — PASS: 17 tests, including Rust-writer/Go-reader and Go-writer/Rust-reader checks plus CRY-006 fixtures.
- Focused `sqlite_contract` — PASS: 8 tests.
- Focused `storage_lifecycle` — PASS: 21 tests, including cross-language contention, rollback, WAL, migration, read-only, and provenance checks.
- Pinned Go profile production replay — PASS: 46 cases from `bf53346eec234929bedf0314b99e3da85dbb991b`, built and run with the available actual Go binary `go1.27.1`; the repository validator's exact `go1.26.6` toolchain was unavailable locally, so `profile_read.py --rust --mutation-check` itself was not executable without downloading that toolchain.
- Rust profile corpus — PASS: 4 tests, including all 46 retained cases and no-write/special-file/property checks.
- `go test -count=1 ./...` on the profile worktree — PASS locally with `go1.27.1`.
- CLI smoke: `cargo run --locked -p symeraseme-cli -- show-profile` — exit 1 with `deferred command: show-profile is not implemented in Rust`.

Remote status read through the unauthenticated GitHub API: PR #921 (head `68fb998ef2568c855cd579dc0438b9d266801d32`, base `e2a4e5cd048603cc83205397f4320a83e8a1034a`) is open. Its Rust fast/native, schema, lint, test, security-scan and SQLite-smoke checks are successful, but `Go / test` is currently failed. Failure logs were not readable without GitHub authentication; the local Go suite passes, so the remote failure is recorded as an unresolved CI state rather than attributed to a source defect. Live `origin/main` checks at `e2a4e5cd048603cc83205397f4320a83e8a1034a` were all successful, with the Rust PR-fast job skipped as expected for main.

## Findings
- [ ] **[Security] V3 HKDF intermediate key material is not zeroized**
  - **Status quo:** `derive_v3_key` stores the HKDF extract output in `pseudorandom_key` and the expand output in `block`, copies `block` into the returned key, and drops both ordinary values at `crates/symeraseme-core/src/storage/encryption.rs:203-215`. The caller zeroizes only the returned key at `:183-185`; the intermediate 32-byte secret values can remain in stack/register spill memory. This is a static-confirmed memory-hygiene gap affecting encrypted event-store key material, relevant if the process is dumped, swapped, or inspected after failure.
  - **Proposed solution:** Keep the extract output and expand block in `zeroize::Zeroizing` containers (or explicitly zeroize them before return), then retain the existing caller-side zeroization. Add a source-level review/test guard that the V3 derivation path does not introduce unzeroized secret buffers and rerun the V3 round-trip and cross-language vectors. No wire-format change is needed.
  - **Effort/Impact:** Effort low; severity medium because exploitation requires post-compromise memory access but the affected values are encryption-derived key material; implementation risk low.

- [ ] **[Architecture] Profile JSON decoding deliberately diverges from Go on UTF-8 and decoder limits**
  - **Status quo:** Rust deserializes decrypted bytes with `serde_json::from_slice` at `crates/symeraseme-core/src/identity/profile.rs:343`, while the frozen contract explicitly records rejection of invalid UTF-8/lone surrogates and bounded-depth parsing at `docs/rust-profile-read-contract.md:77-79`. Go's production path unmarshals the same plaintext at `internal/identity/profile.go:471-484` using `encoding/json`, whose documented behavior replaces invalid UTF-8 and accepts a larger arbitrary JSON domain. The retained 46-case corpus does not cover this divergence. An authenticated profile that Go can read may therefore fail in the Rust consumer, so this is not full Go-compatible profile-read behavior.
  - **Proposed solution:** Decide the consumer contract before CLI-016: either normalize/reject these inputs in the Go production route and add invalid-UTF-8, lone-surrogate, and deep-JSON fixtures to both implementations, or implement a Rust decoder/normalization layer with measured Go parity. Make the chosen behavior an explicit acceptance gate; do not silently claim full profile parity from the current corpus.
  - **Effort/Impact:** Effort medium; severity medium for migration compatibility and potentially high for a user whose only authenticated profile hits the edge case; implementation risk medium because parser behavior and error classification change.

- [ ] **[UX/DX] CLI-016 has no Rust profile adapter despite advertising profile commands**
  - **Status quo:** The Rust command surface advertises `init-profile` with profile fields at `crates/symeraseme-cli/src/command_surface.rs:525-552` and `show-profile` at `:1275-1284`, but `dispatch` handles only version/config/completion/deprecated serve and sends all other paths to `deferred` at `crates/symeraseme-cli/src/cli.rs:333-363`. A real `show-profile` invocation exits nonzero with the deferred-command message. The profile contract itself says no CLI dispatch/template behavior is included at `docs/rust-profile-read-contract.md:23-27`, so this is a confirmed remaining consumer gap, not a failed core-library test.
  - **Proposed solution:** Add one bounded CLI adapter that uses `ProfilePaths::from_process()` and `MasterKeyResolver::from_process()`, maps public profile fields into the existing render context, preserves Go text/JSON/error/exit behavior, and leaves initialization/write semantics to the separately reviewed CLI-016 slice. Add subprocess tests for successful show, missing/corrupt/key errors, output modes, and no secret/plaintext leakage.
  - **Effort/Impact:** Effort medium; severity high for Rust CLI cutover because the advertised profile workflow is unusable; implementation risk medium due output and key-resolution compatibility.

- [ ] **[Architecture] Focused Windows profile gate compiles but does not execute profile behavior**
  - **Status quo:** With `profile_read_only=true`, `rust-ci.yml` runs only a core all-target compile on Windows and exits at `.github/workflows/rust-ci.yml:225-231`; the test, parity, and native profile-read steps are skipped by `:236-251` and `:263-276`. The contract documents this as deferred Windows acceptance at `docs/rust-profile-read-contract.md:103-109`. Thus a successful focused Windows check cannot detect Windows-specific profile path, UTF-8, key, or filesystem behavior; it is compile evidence only. The full default workflow retains broader Windows coverage, but that does not make a focused dispatched result a runtime acceptance result.
  - **Proposed solution:** Before final consumer completion, run the default non-focused Windows native job (or a dedicated Windows profile runtime job) with the Go oracle, Rust corpus, mutation check, and explicit no-write assertions. Keep the focused workflow labeled compile-only and prevent its status from being interpreted as all-platform profile acceptance.
  - **Effort/Impact:** Effort medium/high due Windows runner execution; severity medium as a release-readiness/verification gap, not a demonstrated runtime defect; implementation risk low if isolated to CI gating.

## Open questions / Not verified

The exact pinned Go 1.26.6 runtime and the repository validator's mutation-check path were not locally available because the toolchain is not installed and the review did not perform an automatic download. The exact cause of the current remote PR #921 `Go / test` failure could not be retrieved without GitHub authentication; local Go tests passed. The named CoreKit commit was verified through the EraseMe manifest, lockfile, Cargo resolution and successful compile/tests, but its external source was not reviewed outside this repository scope.

The Rust crypto, SQLite lifecycle, shared CoreKit opening, and retained profile corpus showed no additional confirmed correctness failure in the reviewed paths. This is not whole-product acceptance and does not establish Windows runtime parity or CLI-016 completion.

## Conclusion

Status: unsafe-contract-conflict, not merge-ready for consumer cutover. The bounded crypto/SQLite/profile core slices are locally implemented and pass their focused/native-on-macOS evidence, but full profile parity is explicitly not claimed, the Rust CLI adapter is absent, Windows focused acceptance is compile-only, and PR #921 still has a failed remote Go test check. First next action: resolve the profile decoder contract (including invalid UTF-8/depth fixtures) and implement the bounded CLI-016 read adapter, then rerun the default Windows profile runtime gate and clear the PR Go/test failure with its actual log evidence.
