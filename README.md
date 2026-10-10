# Symaira EraseMe

> **Accepted product direction — implementation pending:** EraseMe remains an independent privacy product. Browse/Operate becoming optional Brain modules and credential UI moving to Brain must not introduce a mandatory Brain dependency into privacy workflows. See [PB-2026-09-09](docs/product-boundaries.md).

[![CI](https://img.shields.io/github/actions/workflow/status/danieljustus/symaira-eraseme/ci.yml?branch=main&label=CI&logo=github)](https://github.com/danieljustus/symaira-eraseme/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/danieljustus/symaira-eraseme?label=Release&logo=github)](https://github.com/danieljustus/symaira-eraseme/releases)
[![License](https://img.shields.io/github/license/danieljustus/symaira-eraseme?label=License)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.98.0-orange)](https://www.rust-lang.org)
[![Swift](https://img.shields.io/badge/swift-5.10%2B-F05138)](https://swift.org)

![Symaira EraseMe social preview](docs/assets/social-preview.png)

Symaira EraseMe helps you exercise your GDPR/CCPA right to erasure against
data brokers. It is a local-first Rust CLI with a native SwiftUI macOS app and
an authenticated MCP JSON-RPC interface.

**Current source tree:** Rust-only CLI and native SwiftUI macOS app, version 0.14.1.
This patch preserves identity keys, enforces single-use consent and completes
all release assets before immutable publication. The Rust-only package layout
was introduced in the
[`v0.14.0` canary prerelease](https://github.com/danieljustus/symaira-eraseme/releases/tag/v0.14.0).
The historical `v0.13.0` prerelease remains dual-backend, and `v0.12.1` is the
last Go-only stable release. Check [GitHub releases](https://github.com/danieljustus/symaira-eraseme/releases)
for published versions and channels. Stable acceptance and the seven-day
post-stable observation are tracked under CUT-004. The Go implementation remains temporarily
as a development oracle until the separately approved CUT-005 retirement.

**Status:** Beta. Core planning, event tracking, deadline handling, registry
validation, inbox triage, reports, and the MCP/CLI contracts are implemented.
Some broker-specific web flows still require manual review.

## Features

- Curated registry of more than 1,200 data brokers with jurisdiction and law metadata.
- Event-sourced SQLite storage with projections and an audit trail.
- CLI commands for profile setup, planning, execution, inbox polling, triage,
  rebuttals, scheduling, reports, and manual fallback tasks.
- Explicit web-form previews and durable manual fallback tasks; browser execution
  is an injected boundary and is not a compile-time dependency.
- Shared Rust LLM provider layer for reply classification and rebuttal generation.
- Local MCP HTTP JSON-RPC server with 26 catalogued tools and Bearer-token auth.
- AES-256-GCM encrypted identity profile, standard Fernet encrypted database at rest, and explicit destructive-operation consent.
- Native SwiftUI dashboard for macOS. Rust-only release DMGs bundle the Rust CLI/MCP server.

## Install

### macOS and Linux

```bash
brew tap danieljustus/tap
brew install symeraseme
symeraseme version
```

### Windows and other platforms

Download the archive for the published version you choose from
[GitHub releases](https://github.com/danieljustus/symaira-eraseme/releases).
Rust-only archives contain only the Rust CLI; no Go rollback binary
or legacy backend selector is distributed. The historical `v0.13.0` prerelease
is dual-backend and its assets must remain unchanged; `v0.12.1` is the last
Go-only stable release. Canary builds reach Homebrew only after stable promotion and the
release gates pass.

### macOS GUI

For a published Rust-only version, download its versioned macOS DMG from
[GitHub releases](https://github.com/danieljustus/symaira-eraseme/releases).
It bundles only the Rust CLI/MCP server. The release workflow requires Developer
ID signing, notarization, stapling and attestation before publication. Local builds may be ad-hoc signed
and are not equivalent to that published artifact. Historical `v0.13.0` assets
remain unchanged. No GUI Homebrew cask is configured.

### Migration from the pre-cutover installation

The last pre-cutover implementation is preserved at the annotated Git tag
`python-final`. Existing installations should migrate their local data before
removing the old runtime:

```bash
symeraseme migrate \
  --source /path/to/old-state \
  --destination /path/to/rust-state \
  --dry-run
```

The migration is explicit, creates a backup before writing, never deletes the
source, and can resume from `.migration-state.json`. See
[TROUBLESHOOTING.md](TROUBLESHOOTING.md) for the full rollback procedure.

## Quick start

```bash
# Create an encrypted local identity profile
symeraseme init-profile

# Inspect the embedded broker registry
symeraseme registry validate
symeraseme brokers list --law GDPR

# Plan, review, and dry-run a campaign
symeraseme plan create --campaign initial --max 5
symeraseme plan show --campaign initial
symeraseme plan execute --campaign initial --dry-run

# Track deadlines and view reports
symeraseme tick --dry-run
symeraseme generate-report
```

Destructive execution requires explicit consent. Review the plan first and use
`symeraseme grant execute --ttl 3600` for a short-lived automation token.

## MCP server

Start the local authenticated HTTP server:

```bash
symeraseme mcp
# Default: http://127.0.0.1:8000
```

Use `--host` and `--port` to change the loopback endpoint. Non-loopback binds
require `--allow-remote`. Use `--stdio` when an MCP client needs
newline-delimited JSON-RPC over standard streams.

The token is generated on startup and written with restrictive permissions to
the configured data directory (`mcp_token`). Send it on every HTTP request as:

```text
Authorization: Bearer <token>
```

Never put the token in source control, issue reports, shell history, or logs.
The complete transport and tool contract lives in
[docs/mcp-contract.md](docs/mcp-contract.md).

## Configuration and secrets

The event store defaults to `~/.local/share/symeraseme/symeraseme.db`; it never
falls back to a temporary directory in production. Configuration precedence is
built-in defaults, `~/.config/symeraseme/config.toml` (or
`$XDG_CONFIG_HOME/symeraseme/config.toml`), the project
`.symeraseme.toml`, then environment variables. `SYMERASEME_DATA_DIR` selects
the base data directory, `SYMERASEME_DB_DIR` overrides only the database
directory, and `SYMERASEME_ENCRYPT_DB` accepts `1|true|yes|on` or
`0|false|no|off`. Invalid values fail closed. When encryption is enabled, the
SQLite file is written as a standard Fernet envelope and decrypted only into a
private temporary directory during use; switching modes is atomic.

Credentials should be referenced through the canonical `symvault://` form or a
platform secure store; resolved values are never logged. Provider-specific
configuration is consumed by the shared Rust LLM provider layer.

## Development

Requirements: Rust 1.98.0 (pinned in `rust-toolchain.toml`). Go 1.26.6 is needed
only for the temporary Go reference/oracle checks (`make go-gate` and live
`make parity-live`); it is not needed to build or run the Rust product. A full
Xcode installation is required for the macOS GUI.

```bash
# Rust CLI and frozen-observation conformance suite
make build-rust
make rust-gate
make parity

# Temporary Go reference checks (development only, not a product runtime)
make go-gate
make parity-live

# macOS app
make app-test
./app/SymairaEraseMe/build.sh

# Local development packaging example only; not a release-version claim
VERSION=0.14.1 ./scripts/package-dmg.sh
```

`make parity` uses recorded Go observations by default and does not require a
Go executable; `make parity-live` rebuilds and runs the Go oracle. `make go-gate`
is the retained Go reference quality/build gate, including its exact 75%
statement-coverage threshold. The Go implementation and tooling remain only for
development/conformance work until the separately scoped CUT-005 retirement.

### Registry contributions

Add a verified YAML broker entry under `registry/brokers/`, then run:

```bash
make build-rust
./build/rust/debug/symeraseme-rust registry validate
```

Do not fabricate endpoints or include personal data. The registry is embedded
in the Rust CLI and is not rewritten by normal CLI operation.

### Project layout

```text
crates/                 Rust CLI, core, and engine
rust-tests/parity/      Conformance suite and frozen Go observations
cmd/symeraseme/,        Temporary Go reference implementation and packages
internal/               (development oracle only until CUT-005)
registry/               Broker/law/schema source data
skills/                 Agent skill bundle and workflow documentation
app/SymairaEraseMe/      SwiftUI macOS client
scripts/                 Build, packaging, and verification tools
```

## Releases

Tags matching `v*` trigger [.github/workflows/release.yml](.github/workflows/release.yml):

1. The Rust prerelease workflow builds natively on Linux, macOS, and Windows
   for amd64 and arm64, packages six static Rust CLI archives, and verifies the
   archives, `checksums.txt` and the per-archive CycloneDX SBOMs.
2. The staging job attests build provenance for the six archives and creates
   a draft prerelease, then reads every asset back without publishing it.
3. The macOS job builds the SwiftUI app with the Rust CLI/MCP server and uploads
   the versioned DMG to that draft. It is signed, notarized, stapled and attested.
4. After all assets and attestations are complete, the final job verifies both
   checksum manifests, publishes the canary and requires GitHub to report it as
   immutable. Enable the repository's **Immutable releases** setting before
   pushing a new tag. Published releases can no longer accept asset changes.
5. When an immutable canary is promoted to stable after its acceptance checks,
   the Homebrew publisher downloads the exact release archives, verifies their
   checksums, and updates `danieljustus/homebrew-tap/Formula/symeraseme.rb`.

No GUI Homebrew cask is configured; the macOS GUI is distributed as a DMG.
Previously published `v0.13.0` dual-backend prerelease assets remain unchanged;
`v0.12.1` is the last Go-only stable release. The `v0.14.0` canary introduced
the Rust-only package layout; version 0.14.1 adds identity/consent safety fixes
and complete-asset immutable publication. Canary publication does not complete CUT-004 stable
acceptance and its seven-day observation, or authorize CUT-005 Go-source
retirement. The historical `python-final` tag remains available for migration
and recovery.

## Documentation

- [Contributing](CONTRIBUTING.md)
- [Troubleshooting](TROUBLESHOOTING.md)
- [MCP contract](docs/mcp-contract.md)
- [Event-store contract](docs/event-store.md)
- [Registry contract](docs/registry-contract.md)
- [Historical Python test classification](docs/go-test-port-classification.md)
- [Agent skill bundle](skills/SKILL.md)

## License

Apache-2.0 — see [LICENSE](LICENSE).
