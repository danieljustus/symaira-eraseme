# Go-to-Rust migration proposal

> **Status:** proposal — implementation is blocked until Daniel answers only `go`
> **Depends on:** #794–#800, approval of this document and the implementation plan
> **Produces:** code, tests, release artifacts, migration documentation

## 1. Why this change

Symaira EraseMe is currently a working, CGO-free Go 1.26.6 application with a
SwiftUI client. Rust is therefore not a rescue operation. The rewrite is only
worth shipping if it preserves every observable contract and yields measured
operational value without weakening portability or rollback.

Baseline captured from `v0.12.1` / commit
`240bf67cefa05e643e32611a02e6e7ed87a033ea` on 2026-09-04:

| Measure | Go baseline |
|---|---:|
| Go source | 123 files / 23,305 physical lines |
| Tests | `make test` passed |
| Lint/vet | passed |
| Coverage | 76.23% (5,225/6,854 statements; gate 75%) |
| arm64 macOS binary | 16,774,642 bytes |
| `version` startup | median 9.30 ms, p95 9.97 ms over 100 runs |
| `version` maximum RSS | 22,282,240 bytes (`/usr/bin/time -l`) |
| Embedded brokers | 1,277 validated by the Go conformance test |
| MCP catalogue | 26 pinned tools |
| Release targets | darwin/linux/windows × amd64/arm64 |

The port must report final measurements against this baseline. Rust being Rust
is not itself a success metric.

Cutover value gate:

- zero first-party `unsafe` and fuzz/property coverage at every untrusted
  parser/protocol boundary;
- no unexplained regression above 20% in p95 startup, maximum RSS, unpacked
  binary size or release archive size;
- at least one measured 15% improvement in p95 startup or maximum RSS, **or**
  an explicit Daniel-approved cost/benefit exception before stable cutover.

## 2. Scope

### Must support

| Capability | Required invariant |
|---|---|
| CLI | Same binary name, command tree, flags, aliases, defaults, output streams, JSON, help/error behavior and exit codes |
| MCP HTTP | Same bind policy, bearer-token file, HTTP statuses, JSON-RPC methods, errors, body limit and shutdown behavior |
| MCP stdio | Newline-delimited JSON-RPC with zero stdout pollution |
| MCP catalogue | Byte-stable 26-tool schema and request fixtures |
| Configuration | defaults → global TOML → project TOML → environment precedence |
| Registry | Same schema v1, strict YAML validation, ordering, filters, embedded data and broker count |
| SQLite | Same schema/user_version, pragmas, ordering, NULL/time behavior, locking and transactions |
| Encryption | Read every shipped Python/Go format, use collision-safe versioning for any corrected write format, and preserve profile/database compatibility as settled by #798 |
| Identity/consent | Same paths, permissions, key lookup, profile format, token hashes, expiry and destructive-operation gates |
| Domain behavior | Campaigns, replies, deadlines, reports, scheduler, redaction, templates and migration command |
| External boundaries | SMTP, IMAP, OAuth2, LLM providers and the production inbox/web-form behavior corrected by #799/#800 |
| SwiftUI app | No protocol changes required; it still launches `symeraseme mcp` and reads `mcp_token` |
| Distribution | Same archive names/layout, checksums, Homebrew installation, DMG layout/signing and supported targets |
| Rollback | Last known-good Go backend remains runnable and shippable through one stable Rust release |

### Out of scope

| Item | Reason |
|---|---|
| New user-visible features | A rewrite must not hide feature work |
| MCP schema changes or tool renames | Contract change, not implementation change |
| SQLite schema v2 | Storage migration is an independent risk |
| New browser automation | Current Go default/manual fallback is the oracle; a new `symbrowse` protocol needs a separate proposal |
| Template wording changes | Legal output must remain byte-stable |
| SwiftUI redesign | The app is a consumer, not part of the language port |
| Deleting Go during implementation | Go remains the executable oracle and rollback path |
| A cross-repository Cargo workspace | Violates repository independence |

## 3. Delivery model

The repository becomes a temporary dual-language repository:

1. Go remains the default `symeraseme` implementation.
2. Rust builds as `target/.../symeraseme`; parity tooling copies it to an
   isolated name/path so both binaries can run in one test.
3. Every vertical slice adds black-box Go↔Rust comparisons.
4. A prerelease ships Rust as the primary `symeraseme` plus a sibling
   `symeraseme-go` fallback.
5. `SYMERASEME_BACKEND=go` makes the Rust entrypoint replace itself with the
   sibling fallback on Unix and spawn/wait with inherited stdio plus exact exit
   propagation on Windows. The fallback is documented, tested and never
   selected silently.
6. After one stable Rust release without unexplained parity defects, Go removal
   is a separate reviewed PR. The last Go tag and rollback instructions remain.

The SwiftUI app keeps the same subprocess and MCP boundary. Only build/package
scripts change from `go build` to Cargo after cutover.

## 4. Architecture and framework choice

### Chosen layout

An internal Cargo workspace with three crates:

```text
Cargo.toml
rust-toolchain.toml
crates/
  symeraseme-core/      # domain, registry, storage, crypto, identity
  symeraseme-engine/    # email, LLM, campaign/replies orchestration, scheduler
  symeraseme-cli/       # clap binary, MCP HTTP/stdio, command dispatch
rust-tests/
  parity/               # black-box cases and fixture manifests
```

No crate is published during the port. Public APIs exist only where the three
internal crates need them.

### Alternatives

| Approach | Parity isolation | Build complexity | Long-term shape | Risk | Verdict |
|---|---:|---:|---:|---:|---|
| One large crate | medium | low | weak boundaries | medium | rejected |
| Three internal crates | high | medium | clear domain/I/O/surface split | low-medium | **chosen** |
| One crate per Go package | high | high | transliterates Go architecture | high | rejected |
| New shared Rust corekit first | high | very high/cross-repo | reusable but speculative | high | rejected for now |
| Rust wrapper around Go libraries | low | high/FFI | keeps two runtimes | high | rejected |

### Initial Rust stack

Exact versions are selected from current stable releases and committed in
`Cargo.lock` when implementation starts; no floating Git dependencies.

- CLI: `clap`
- serialization: `serde`, `serde_json`, `toml` and a maintained
  Serde-compatible YAML parser selected by the full 1,277-entry corpus gate
  (do not adopt the archived `serde_yaml` crate by habit)
- errors/logging: `thiserror`, `anyhow` only at binary boundaries, `tracing`
- SQLite: `rusqlite` with bundled SQLite after native-target build proof
- crypto: RustCrypto primitives selected after #798 (`aes`/CBC/PKCS7 for
  standard Fernet; `aes-gcm` only for a separately versioned legacy/current-Go
  compatibility path; plus `hkdf`, `pbkdf2`, `scrypt`, `hmac`, `sha2`)
- HTTP: `tokio`, `axum`, `tower`, `reqwest` with rustls
- keyring: `keyring`, tested natively on all supported platforms
- email: crate choice is gated by protocol fixtures and maintenance status;
  no crate is accepted merely because it compiles
- templates: `minijinja` may read the existing Jinja sources only if all 11
  golden outputs are byte-identical; otherwise dedicated Rust templates stay
  frozen to the Go output
- testing: `assert_cmd`, `predicates`, `insta`, `proptest`, `httpmock`,
  `cargo-nextest`, `cargo-llvm-cov`, `cargo-fuzz`

Existing projects were checked before choosing custom infrastructure:
`trycmd`/`snapbox` are suitable for focused CLI snapshots but do not replace
the required cross-binary filesystem/SQLite/network harness; the official
`modelcontextprotocol/rust-sdk` (`rmcp`) is the conformance reference but may
not rewrite EraseMe's pinned legacy frames; `cargo-dist` is evaluated only if
it reproduces the existing archive, checksum, Homebrew and DMG contracts
without parallel release machinery.

Every production crate starts with `#![deny(unsafe_code)]`.

## 5. Dependency decisions

### `symaira-corekit`

Rust cannot import the Go package. Required behavior is re-expressed behind
local Rust adapters. LLM provider descriptors remain sourced from
`symaira-corekit/contracts/llm_providers.json`: the port commits a generated
artifact with source tag/SHA metadata and a drift check. Provider semantics are
not manually forked.

### `symaira-browse/formflow`

The current Go code imports the Go library, but the shipped handler injects no
driver and non-dry-run execution cannot automate a form. Issue #800 must first
settle and test the honest production boundary: stable runtime integration or
explicit manual fallback. Rust ports that corrected contract exactly; it does
not invent a third behavior during the rewrite.

### SQLite portability

`rusqlite` bundles C SQLite and is not equivalent to pure-Go modernc SQLite.
Native Linux, macOS and Windows jobs must compile and run database tests. Cross
compilation alone is not acceptance. If static target requirements cannot be
met without fragile tooling, the cutover aborts rather than silently reducing
target support.

## 6. Implementation stages

The executable dependency/order contract is
`docs/plans/2026-09-04-go-to-rust-task-graph.json`; the detailed actions are in
the companion implementation plan.

1. **Gate and freeze** — baselines, complete contract matrix, oracle build,
   #794–#800 resolved before their affected gates, toolchain/crate proof.
2. **Shadow foundation** — Cargo workspace, Rust CI, black-box harness, version,
   config, time and command-shell contracts.
3. **Data contracts** — registry, templates, redaction and deterministic pure
   domain logic.
4. **Storage/security** — SQLite, projections, file locking, encryption,
   identity and consent interoperability.
5. **External adapters** — SMTP/IMAP/OAuth2, LLM descriptors/transports and
   deterministic mock-server tests.
6. **Application services** — campaigns, replies, deadlines, reporting,
   scheduler, manual tasks and legacy migration command.
7. **Protocol surface** — full CLI and MCP HTTP/stdio parity, fuzz/property
   tests, Swift app integration.
8. **Release shadowing** — native six-target artifacts, SBOM/audit/deny,
   Homebrew/DMG dry runs, signed/notarized test artifact.
9. **Reversible cutover** — prerelease, bundled Go fallback, explicit canary,
   stable Rust release.
10. **Retirement** — remove Go only in a later PR after the observation gate.

Each stage is mergeable while the Go product remains fully working.

## 7. Gates

- **Pre-flight gate:** clean `main`, green Go suite, frozen baseline and approved
  plan. Failure stops all coding.
- **Revision gate:** each slice must pass unit tests, Go↔Rust differential cases,
  formatting, Clippy and affected native tests. Mismatches are defects until
  explicitly classified.
- **Escalation gate:** any intended contract change, unsupported platform,
  unsafe Rust, schema change or weaker secret handling returns to Daniel.
- **Abort gate:** inability to read existing encrypted data, corruption or loss
  risk, stdout pollution, reduced release targets, or no credible rollback.
- **Cutover gate:** all rows in `docs/rust-port-contract-matrix.md` are green,
  full local/CI/release gates pass, Swift app works against Rust, and fallback
  has been exercised.

## 8. Risks

| Risk | Severity | Control |
|---|---|---|
| Python uses standard Fernet while Go reused headers for a custom token | critical | resolve #798 first; generate Python/Go vectors and version incompatible bytes distinctly |
| Contract currently misstates header lengths | high | resolve #795; raw Go-generated vectors override hand-entered lengths |
| Production Go bypasses DB-dir/encryption config | critical | resolve #796 before freezing the Go oracle; test persistent and encrypted restart behavior |
| Swift `listTools()` expects the wrong response envelope | high | resolve #797 and freeze the real `tools/list` shape before app parity |
| `poll_inbox` has no production dialer/HWM | high | resolve #799 and freeze a fake-server transcript before the email slice |
| `run_web_form` has no production driver | high | resolve #800; choose an honest runtime integration or manual fallback before the campaign slice |
| Whole-file encrypted SQLite + WAL | critical | checkpoint/close/copy/re-encrypt crash tests; never operate on real user data in tests |
| Keyring differences | high | native platform tests and explicit fallback-order fixtures |
| Cobra vs Clap text/errors | high | snapshot every command/help/error/exit code |
| YAML and map ordering | high | full 1,277-entry corpus plus semantic/byte modes per contract |
| Go-only shared packages | high | narrow local adapters and generated SSOT artifacts |
| SMTP/IMAP crate behavior | high | protocol transcript tests before selecting crates |
| Release regression | high | preserve exact asset matrix/names and test archive contents |
| Scope/effort | high | vertical slices; stop if parity cost exceeds value |

## 9. What this replaces

After acceptance, this proposal supersedes Go-specific implementation language
in README/development/release documentation. It does **not** replace
`docs/mcp-contract.md`, `docs/event-store.md`, `docs/registry-contract.md`, the
SwiftUI app contract, or historical `docs/go-test-port-classification.md`.

## 10. Approval

Daniel's single reply `go` means:

- accept the architecture, scope, gates and reversible cutover above;
- authorize autonomous step-by-step implementation using isolated worktrees and
  subagents;
- authorize task branches, local commits, pushes, PR creation, CI repair and
  verified squash merges through completion; no silent contract changes;
- preserve unrelated WIP and never use personal data or real credentials in
  tests.

No implementation begins before that reply.
