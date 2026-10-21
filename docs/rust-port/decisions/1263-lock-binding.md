# Decision 1263: whole-Cargo.lock binding of frozen native captures

Status: decided — keep the strict rule. No code narrowing in this change.
Base: `2b61192feb64dd0e7507f7ef40453b4c63d62035`.
Related: #1188 (strict binding retained), #1194 (refresh without weakening binding), #1257 (Dependabot group closed on this binding), #1232 (Go retirement).

## Question

Can `go_source_pin::assert_current_matches_archive` stop requiring the whole current
`Cargo.lock` to equal the archived lock now that Go is retired, so that a dependency
bump no longer needs a full six-target capture refresh?

## Evidence

Rule as implemented (`crates/symeraseme-cli/tests/support/go_source_pin.rs`):

- `:122-126` contract: authenticate the archive by recorded size and digest first; for
  `Cargo.lock`, only the four first-party `version` lines may differ.
- `:127-131` any non-`Cargo.lock` input must equal the archive byte-for-byte.
- `:140-162` rewrites the four local package stanzas (`parity`, `symeraseme-cli`,
  `symeraseme-core`, `symeraseme-engine`) to `CARGO_PKG_VERSION`.
- `:163-166` the rest of the lock must be byte-identical (dependencies, checksums,
  line endings).

Call sites (eight production readers plus the self-test):

- `crates/symeraseme-cli/tests/support/frozen_plan_process.rs:92`
- `crates/symeraseme-cli/tests/support/frozen_native_triage.rs:204`
- `crates/symeraseme-cli/tests/support/frozen_http_aux.rs:414`
- `crates/symeraseme-cli/tests/support/frozen_http_wire.rs:107`
- `crates/symeraseme-cli/tests/support/frozen_agent_error.rs:147`
- `crates/symeraseme-cli/tests/support/frozen_http_headers.rs:138`
- `crates/symeraseme-core/tests/support/frozen_smtp.rs:202`
- `crates/symeraseme-cli/tests/go_source_pin.rs:171` (negative controls, `only_first_party_release_lock_versions_may_differ`)

Cargo.lock is not a Go input. It is recorded as an **archived generator**:
`tests/fixtures/go-frozen/*/**.json` key `/archived_generators/Cargo.lock`
(e.g. `refresh-411b5c10`: 82067 bytes, sha256 `50a81428…`). The Rust producers and
readers that consume these captures import dependencies whose versions can change
recorded bytes or the code path that produces them:

- `crates/symeraseme-core/tests/campaign_smtp.rs:3-5` — `chrono`, `serde_json`, `sha2`
- `crates/symeraseme-core/tests/smtp_transport.rs:3-4` — `base64`, `serde_json`; `:26-28` — `rcgen`, `rustls` (certificate and TLS handshake material)
- `crates/symeraseme-cli/tests/plan_execute_live_process.rs:4` — `serde_json`
- `crates/symeraseme-core/tests/support/frozen_smtp.rs:141` — `sha2` digests of recorded source bytes

Current state: the active reader fixture `refresh-e61132f8` records a lock of 80762
bytes, sha256 `05c1fcaa…`. That equals the base lock exactly, so the strict rule
passes today without relying on the local-version allowance. Six lock-changing
commits sit between `5df99f8f` and base (`#1194`, `#1215`, `#1228`, `#1247`, `#1248`,
`#1261`); each refreshed captures rather than relaxing the check.

Policy: `docs/rust-port/dependency-refresh-policy.md` — the strict rule is retained
at `:20-21`, the native-capture refresh procedure at `:10-25`, and the publication
privacy derivation at `:40-70` explicitly does not relax it (`:56-57`, `:69-70`).

Go retirement: `RETIREMENT_SOURCE_REVISION = 22615ae…` (`go_source_pin.rs:21`) keeps
recorded Go and generator bytes authenticated from history. It does not reduce the
dependency surface of the Rust producers, so it does not by itself justify dropping
the lock binding.

## Options

1. **Keep the strict whole-lock rule (chosen).** Cost accepted: every dependency
   change needs a six-target native capture refresh and readers unlock only after it
   (the #1257 failure mode).
2. **Narrow to a named input set.** Bind only the lock entries reachable from the
   producer/reader test targets (via `cargo metadata` reachability over the lock
   graph), keep local-version rewrite, keep all other lock bytes pinned for those
   targets. Rejected for this slice: the reachability proof does not exist; producers
   import `rcgen`/`rustls`/`serde_json`/`sha2`/`base64`/`chrono` and their output can
   depend on those versions; the closure computation is not contained to
   `go_source_pin.rs`; and it weakens generator integrity, which #1194 and #1188
   explicitly preserved.
3. **Drop the lock binding.** Rejected: removes generator-integrity authentication
   and the negative control that a changed lock is rejected.

## Risks

- Option 1: dependency PRs stay unmergeable until a full native refresh (cost, not
  a correctness risk). Mitigation: batch dependency bumps into one refresh cycle.
- Option 2 (future): a wrong reachability closure silently un-pins a generator input.
  Would need a per-target proof artefact and negative controls for each pinned
  producer. Not attempted here.
- Option 3: loses provenance for captures; ruled out.

## Chosen option

Keep the strict rule. Implementation: none. The go_source_pin code is unchanged.
Stage 2 (narrowing) is not started; it is listed as remaining work below.

## Future narrowing preconditions (for a later, separately approved decision)

- A reproducible closure artefact generated from `cargo metadata` for every producer
  and reader test target, committed and digest-bound.
- Proof that every pinned-out package cannot affect recorded bytes (or a demonstration
  that each producer's output is byte-identical across the candidate lock).
- Negative controls for a changed bound input (must still fail) and a changed
  unbound input (must still pass).
- Native six-target acceptance on the narrowed binding.

## Reviewer checklist

- [ ] `go_source_pin.rs` is unchanged by this commit (`git diff --stat HEAD~1` shows only doc files).
- [ ] The eight call sites above still route through `assert_current_matches_archive`.
- [ ] `RETIREMENT_SOURCE_REVISION` and the four local-version labels are unchanged.
- [ ] `refresh-e61132f8` archived-lock digest equals the base lock digest (`05c1fcaa…`).
- [ ] No frozen capture, fixture, CI file or Cargo manifest was modified.
- [ ] The policy document's decision section points to this record and does not relax `#1188`.
