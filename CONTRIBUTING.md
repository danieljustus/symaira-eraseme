# Contributing to Symaira EraseMe

## Quick start

1. Clone the repository and create a feature branch.
2. Build and verify the Rust CLI candidate:

   ```bash
   python3 scripts/prepare-frozen-history.py --fetch
   make build-rust
   make rust-gate
   ```

3. For the macOS GUI, use a full Xcode installation:

   ```bash
   make app-test
   ./app/SymairaEraseMe/build.sh
   ```

4. Open a pull request with a focused change and the relevant issue reference.

This source tree includes the Go-source retirement candidate, not a claim of
published retirement acceptance. Historical `v0.13.0` remains a dual-backend
prerelease; `v0.12.1` is the last Go-only stable release. Frozen Go observations
and their immutable source history remain as conformance evidence; normal
builds and tests require no Go installation.

Use a full-history clone. Before the first test run, explicitly prepare the
eight immutable producer commits with the command above. A squash merge and
feature-branch deletion can leave these objects outside normal branch/tag
history, even with `fetch-depth: 0`. Preparation downloads only missing pinned
objects from the public upstream repository, without changing the checkout,
fixtures or recorded source identities. Without `--fetch`, the command only
checks availability and fails if an object is missing. Subsequent tests remain
offline with respect to this history; they never fetch or bypass provenance.

## Adding a data broker

Adding or updating a broker requires verified public evidence. No personal data
or unverified opt-out endpoint belongs in the registry.

### Required fields

```yaml
id: example-broker-us
name: Example Broker Inc.
website: https://example.com
category: people-search
jurisdictions: [US]
laws: [CCPA]
data_sensitivity: 3
priority: medium
status: active
opt_out:
  - type: email
    endpoint: privacy@example.com
    template: ccpa-deletion
    locale: en
    required_fields: [full_name, email]
    expected_response_days: 45
```

- Put the file under `registry/brokers/us/`, `registry/brokers/eu/`, or
  `registry/brokers/uk/` according to its primary jurisdiction.
- Name it `<broker-id>.yaml`; the `id` must equal the file stem.
- Include a reliable `source` and verification keywords where available.
- Keep one broker addition or update per pull request.

Validate the embedded registry before opening the pull request:

```bash
make build-rust
./build/rust/debug/symeraseme-rust registry validate
```

## Code contributions

- Rust is the current source-tree product implementation. Keep CLI and MCP
  behavior compatible with `docs/mcp-contract.md` and use `make rust-gate` for
  the full Rust check.
- Use `make parity` for frozen-observation conformance. Preserve recorded
  observations, immutable source pins and negative controls; do not regenerate
  expected behavior from the Rust candidate or bypass provenance checks.
- Keep secrets as references or environment configuration. Never log resolved
  secret values or commit credentials.
- Use the existing event-store and registry abstractions instead of adding
  parallel storage or schema formats.
- Swift changes must build with the full Xcode toolchain and keep the app's
  `com.symaira.eraseme` bundle contract intact.

## Testing

| Area | Command | Scope |
|---|---|---|
| Rust CLI build | `make build-rust` | Build the current Rust CLI candidate |
| Rust full gate | `make rust-gate` | Format, workspace check, Clippy, tests, and doctests |
| Frozen-observation parity | `make parity` | Compare Rust behavior with recorded Go observations |
| Rust coverage | `make coverage` | cargo-llvm-cov; 80% project lines and 90% aggregate critical-code lines |
| Rust interpreter | `make miri` | Pinned nightly Miri contract, not whole-program race detection |
| macOS GUI tests | `make app-test` | Swift package tests with full Xcode |
| macOS app build | `./app/SymairaEraseMe/build.sh` | Build the app and stage its Rust CLI/MCP server |

Do not weaken an assertion to make a test pass. When a test reveals a
compatibility or byte-format mismatch, fix the implementation or document the
intentional contract change.

`make rust-gate` does not replace native platform, coverage, Miri, dependency
security or release-artifact acceptance. `make test-race` fails explicitly:
the retired Go race detector is not silently relabelled as a Rust check.

## CI icon compilation

Pull requests classify the inputs listed in `.github/workflows/ci.yml` on Linux
before requesting a macOS runner for `app icon compile`. Changes to the native
icon package, `.icns` file, checksum manifest, icon test/verifier scripts, or
that workflow still run native compilation. Confirmed unrelated changes, such
as documentation edits, skip only this icon job. Missing history, selector
failure, or unknown output does not authorize a skip. Main pushes, manual runs,
and scheduled runs retain native icon compilation; other CI gates are unchanged.

## Pull requests

PR descriptions should state:

- what behavior changed and why;
- which issue is addressed;
- the exact local checks run;
- any platform-specific or signing limitation.

Keep generated `dist/`, local coverage profiles, and build output out of the
commit. GitHub Actions workflows must use pinned action SHAs and least-privilege
permissions.

## Release changes

Release configuration for the current Rust-only candidate lives in
`.github/workflows/release.yml`; it builds Rust CLI archives and the macOS app.
The historical `v0.13.0` prerelease remains dual-backend, and `v0.12.1` is the
last Go-only stable release. The obsolete GoReleaser and golangci-lint
configurations are removed; their historical Git versions remain available.
The Homebrew publisher consumes exact published archives and checksums. Do not create or move
a release tag from a feature branch; use the repository's release gate and
verify the release assets and Homebrew Formula after publication.

### Local Rust archive dry-run

With Python 3.11+ and already-built native Rust binaries for all three supported targets:

```bash
make release-dry-run \
  RELEASE_BINARIES=/path/to/native-binaries \
  RELEASE_DIST=/path/to/empty-output
```

The input directory must contain `rust-binary-<os>-<arch>` for each
of `darwin-arm64`, `linux-amd64`, `linux-arm64`, matching the native
`rust-prerelease.yml` artifacts. The target checks executable headers, stages
three archives and verifies their root layout and SHA-256 checksums. It neither
builds binaries nor downloads or publishes anything. Missing or wrong-target
inputs and non-empty output directories fail without cleaning existing output.

Defaults are `$(RUST_TARGET_DIR)/release-binaries` and
`$(RUST_TARGET_DIR)/release-dist`. The archive version defaults to
`Cargo.toml`'s workspace version. An explicit `RELEASE_VERSION` changes the
archive label only; it does not verify the executable's version or provenance.
This packaging check does not replace the three native builds, version/smoke,
static-dependency, SBOM, signing or release-acceptance gates.
