# Troubleshooting Symaira EraseMe

## Binary not found

Install the static CLI from the Symaira Homebrew tap or download the archive
for the current operating system and architecture from the GitHub release:

```bash
brew tap danieljustus/tap
brew install symeraseme
symeraseme version
```

For a source build of the current Rust-only candidate, run `make build-rust` from
the repository root. The binary is `build/rust/debug/symeraseme-rust`; this
candidate is not yet a published Rust-only release.

## MCP server cannot start

The supported server command is `symeraseme mcp`. It binds to loopback on port
8000 by default and writes its authentication token below the configured data
directory.

```bash
symeraseme mcp --host 127.0.0.1 --port 8000
symeraseme mcp --stdio
```

Non-loopback HTTP binds require an explicit `--allow-remote`. A `401` response
means the client did not send the token; a connection refusal means the server
is not listening. Keep the token file private and never paste it into logs or
issue reports.

## Identity and data directory

Initialize the encrypted profile before planning a campaign:

```bash
symeraseme init-profile
symeraseme show-profile --output json
```

The Rust source-tree candidate defaults to
`~/.local/share/symeraseme/symeraseme.db`. `SYMERASEME_DATA_DIR` is optional and
overrides the base data directory when you need an isolated or non-default
location. Database encryption is configurable with `SYMERASEME_ENCRYPT_DB`;
credentials should be referenced through the canonical `symvault://` form or a
platform secure store.

## Migrating an older installation

The migration command is explicit and dry-run first:

```bash
symeraseme migrate \
  --source /path/to/old-state \
  --destination /path/to/rust-state \
  --source-config /path/to/old-config \
  --destination-config /path/to/rust-config \
  --dry-run
```

The source is never deleted. A complete backup is created before writes, and
`.migration-state.json` allows an interrupted migration to resume. Keep the
backup until the new CLI has been exercised successfully.

## Web forms and manual tasks

The current Rust CLI/MCP path has no browser executor. `run-web-form` with
`--dry-run` previews the registry form; a non-dry run without an injected
executor creates a durable `manual_tasks` entry with `reason=dynamic_form`. It
does not open a browser, submit the form, or report success. `auto-confirm`
also creates a linked manual task when no executor is injected; no confirmation
link is clicked.

```bash
symeraseme run-web-form <broker-id> --dry-run
symeraseme run-web-form <broker-id>
symeraseme manual-tasks list
symeraseme manual-tasks show <task-id>
symeraseme manual-tasks complete <task-id> --notes "Completed manually"
```

Mark a task complete only after the user has performed and confirmed the manual
action. Do not submit a real opt-out while debugging unless the user explicitly
asked for that action and the target data is appropriate for the operation.

## Email and triage

Inbox polling requires an IMAP account and a platform-appropriate credential.
Use a dry-run or isolated account while configuring it:

```bash
symeraseme poll-inbox --host imap.example.com --port 993 --username <address>
symeraseme classify-reply <request-id>
symeraseme generate-rebuttal <request-id>
```

LLM-backed classification and rebuttal use the Rust provider layer. Configure a
provider only when using those features: `anthropic` requires
`ANTHROPIC_API_KEY`, `openai` requires `OPENAI_API_KEY`, Ollama can use
`OLLAMA_HOST`, and `openai-compatible` requires `SYMERASEME_LLM_BASE_URL`. The
`agent` provider uses its configured local agent backend. These are not global
CLI prerequisites; keep provider credentials out of issues and logs.

## Scheduler

Generate files first, inspect them, then install only after review:

```bash
symeraseme generate-scheduler --platform launchd --output ./schedules
symeraseme schedule status --platform launchd
```

The scheduler supports cron, launchd, and systemd. Activation can affect future
outbound actions; use `--dry-run` wherever available and preserve generated
files for diagnosis.

## macOS GUI and DMG

The GUI requires a full Xcode installation, not Command Line Tools alone. For a
local DMG from the current source-tree candidate, the package version in
`Cargo.toml` is `0.14.0`; this is an unreleased candidate version, not a claim
that `v0.14.0` has shipped. Build from the repository root with full Xcode:

```bash
./app/SymairaEraseMe/build.sh
VERSION=0.14.0 ./scripts/package-dmg.sh
```

The current candidate app bundle contains `Symaira EraseMe.app/Contents/MacOS/symeraseme`.
A local build may be ad-hoc signed; the release workflow records whether Developer
ID signing, notarization, and stapling were completed. An ad-hoc DMG is not
Gatekeeper-ready.

## Windows source build

The current Rust-only source-tree candidate builds on Windows with the Rust
CLI target: `cargo build -p symeraseme-cli --bin symeraseme-rust`. The release
workflow defines Windows target builds. Go cross-builds are for the retained
development oracle, not the current candidate product.

## Reporting a problem

Include the command, operating system, CLI version, and a redacted error. Do
not include profile files, token files, API keys, email contents, or personal
identity data. Security vulnerabilities belong in the repository's private
security reporting channel.
