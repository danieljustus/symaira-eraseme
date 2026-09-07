# Issue #862 — CoreKit Rust version adoption evidence

- **Issue:** #862 (`Closes #862`)
- **CoreKit work item:** RUST-005 / symaira-corekit#232
- **Adopter:** `symaira-eraseme`, Rust workspace crate `symeraseme-core`
- **Adopted crate:** `symaira-core-version`
- **Pin:** `git+https://github.com/danieljustus/symaira-corekit` at revision `27177f25f551cecefa7bd6c4524abf175b3a75c7`
- **Lockfile:** `Cargo.lock` records the same full revision in the `source` field.

## Adoption change

`symeraseme-core::version` now re-exports CoreKit's `Info` and `WriteError`,
constructs the tool-specific payload through CoreKit's `new`, and delegates JSON
writing and display formatting to CoreKit. The EraseMe constants and `current`
entry point remain available. The CLI uses the thin `json_line` and `text`
adapters, so the existing public command surface is unchanged.

The local `VersionInfo` struct and its `serde_json::to_vec`/newline renderer
were removed. The measured duplicate block in the pre-change
`crates/symeraseme-core/src/version.rs` was 31 physical lines (24 nonblank,
non-comment source LOC; the block included the serde import, struct, and
methods). The file changed from 75 to 58 total lines. `serde_json` moved from
this crate's runtime dependencies to dev-dependencies for existing parity
fixtures; production version serialization is supplied by the pinned crate.

## Dependency and feature closure

Observed with `cargo tree -p symaira-core-version -e features --depth 3`:

- `serde` 1.0.229: default + derive
- `serde_json` 1.0.151: default/std + `arbitrary_precision` + `raw_value`
- no `thiserror`/proc-macro dependency; `WriteError` uses equivalent manual standard-library implementations
- No HTTP, async runtime, filesystem, SQLite, or other product-specific feature
  enters through `symaira-core-version`.

`cargo tree -p symeraseme-core --depth 2` confirms the consumer's runtime
closure is `chrono`, `serde`, `symaira-core-version`, and `toml`; `proptest` and
`serde_json` are test-only entries.

## Executed evidence

All commands below exited 0 on the assigned macOS arm64 host unless noted:

```text
make test
make test-race
make lint
make build
make vet
make fmt-check
make rust-gate
make parity
cargo fmt --all --check
CARGO_TARGET_DIR=build/rust cargo check --workspace --all-targets
CARGO_TARGET_DIR=build/rust cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=build/rust cargo test --workspace --all-targets
CARGO_TARGET_DIR=build/rust cargo test --workspace --doc
CARGO_TARGET_DIR=build/rust cargo metadata --locked --no-deps --format-version 1
CARGO_TARGET_DIR=build/rust cargo test -p symeraseme-core --lib --test config_parity
CARGO_TARGET_DIR=build/rust cargo test -p symeraseme-cli --test version
```

The focused CLI suite reported 8 passing version tests, including exact
`--version`, plain `version`, `version --json`, negative-argument behavior, and
poisoned-environment cases. The Rust parity target reported 21 unit tests and
1 differential test passing. The Go version regression tests also passed:

```text
go test ./cmd/symeraseme -run 'TestVersion' -count=1
```

Standalone startup was exercised with an empty environment and temporary HOME:

```text
env -i PATH="$PATH" HOME="$TMP_HOME" build/rust/debug/symeraseme-rust --version
env -i PATH="$PATH" HOME="$TMP_HOME" build/rust/debug/symeraseme-rust version
env -i PATH="$PATH" HOME="$TMP_HOME" build/rust/debug/symeraseme-rust version --json
```

Observed output:

```text
symeraseme version 0.13.0
symeraseme 0.13.0
{"tool":"symeraseme","version":"0.13.0","schema_version":1}
```

`VERSION=0.13.0 make build-go` followed by the same three sanitized process
invocations produced the same visible payloads. The Rust integration test
`version_json_matches_handshake_bytes` verifies the contractual trailing
newline byte-for-byte.

## Measurement boundary

No startup p95/RSS/binary-size canary or clean/warm build distribution is
claimed here. Those RUST-005 measurements were not run for this single
consumer adoption; no benchmark values were fabricated. The app gate was
attempted with `make app-test` but exited 2 before running Swift tests because
the host has Command Line Tools selected rather than a full Xcode installation
(`xcodebuild` is unavailable).