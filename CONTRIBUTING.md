# Contributing to Symaira EraseMe

## Quick start

1. Clone the repository and create a feature branch.
2. Build and verify the Rust CLI candidate:

   ```bash
   make build-rust
   make rust-gate
   ```

3. For the macOS GUI, use a full Xcode installation:

   ```bash
   make app-test
   ./app/SymairaEraseMe/build.sh
   ```

4. Open a pull request with a focused change and the relevant issue reference.

The current source tree is the Rust-only candidate and is not yet a published
release. Published `v0.13.0` remains a dual-backend prerelease; `v0.12.1` is the
latest stable release. Go is retained only as a development oracle for
conformance checks until the separately scoped CUT-005 retirement.

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
- Go code is retained as a development oracle. Keep it CGO-free and use
  `make go-gate`; use `make parity-live` when a change needs comparison with the
  live Go oracle.
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
| Live oracle parity | `make parity-live` | Rebuild and compare with the Go development oracle |
| Go oracle checks | `make go-gate` | Go format, tests, lint, vet, 75% coverage, and oracle build |
| macOS GUI tests | `make app-test` | Swift package tests with full Xcode |
| macOS app build | `./app/SymairaEraseMe/build.sh` | Build the app and stage its Rust CLI/MCP server |

Do not weaken an assertion to make a test pass. When a test reveals a
compatibility or byte-format mismatch, fix the implementation or document the
intentional contract change.

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
The published `v0.13.0` prerelease remains dual-backend, and `v0.12.1` is the
latest stable release. `.goreleaser.yml` and `make release-dry-run` are retained
legacy Go packaging/oracle tooling, not the current release path. The Homebrew
publisher consumes exact published archives and checksums. Do not create or move
a release tag from a feature branch; use the repository's release gate and
verify the release assets and Homebrew Formula after publication.
