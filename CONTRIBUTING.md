# Contributing to Symaira EraseMe

## Quick start

1. Clone the repository and create a feature branch.
2. Build and verify the Go CLI:

   ```bash
   make build
   make test
   make lint
   make coverage
   ```

3. For the macOS GUI, use a full Xcode installation:

   ```bash
   ./app/SymairaEraseMe/build.sh
   ```

4. Open a pull request with a focused change and the relevant issue reference.

The supported runtime is the static `symeraseme` binary. The macOS GUI is a
Swift Package Manager application that starts the Go MCP server bundled in the
release app. No external runtime is required for released artifacts.

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
make build
./symeraseme registry validate
```

## Code contributions

- Go code must remain CGO-free (`CGO_ENABLED=0`) and compile on Linux, macOS,
  and Windows for the supported architectures.
- Use `gofmt`, `go vet`, and the configured `golangci-lint` checks.
- Preserve the CLI and MCP contracts in `docs/mcp-contract.md`.
- Keep secrets as references or environment configuration. Never log resolved
  secret values or commit credentials.
- Use the existing event-store and registry abstractions instead of adding
  parallel storage or schema formats.
- Swift changes must build with the full Xcode toolchain and keep the app's
  `com.symaira.eraseme` bundle contract intact.

## Testing

| Area | Command | Scope |
|---|---|---|
| Go unit tests | `make test` | All Go packages |
| Go race tests | `make test-race` | All Go packages with the race detector |
| Go coverage | `make coverage` | Exact 75% statement gate |
| Go static checks | `make lint && make vet` | Formatting, lint, and vet |
| CLI cross-build | `GOOS=windows GOARCH=amd64 go build ./cmd/symeraseme` | Platform compilation |
| macOS GUI | `cd app/SymairaEraseMe && swift test` | Swift package tests |
| macOS packaging | `VERSION=0.12.0 ./scripts/package-dmg.sh` | App bundle and DMG path |

Do not weaken an assertion to make a test pass. When a test reveals a
compatibility or byte-format mismatch, fix the implementation or document the
intentional contract change.

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

Release configuration lives in `.goreleaser.yml` and
`.github/workflows/release.yml`. GoReleaser produces static CLI archives; the
Homebrew workflow writes `Formula/symeraseme.rb` from the exact published
archives and checksums. The macOS job builds the versioned DMG and records
whether Developer ID signing, notarization, and stapling were completed.

Do not create or move a release tag from a feature branch. Use the repository's
release gate and verify the GitHub release assets and Homebrew Formula after
publication.
