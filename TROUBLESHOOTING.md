# Troubleshooting Symaira EraseMe

## Binary not found

Install the static CLI from the Symaira Homebrew tap or download the archive
for the current operating system and architecture from the GitHub release:

```bash
brew tap danieljustus/tap
brew install symeraseme
symeraseme version
```

For a source build, run `make build` from the repository root and ensure the
resulting `symeraseme` binary is on `PATH`.

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

Use `SYMERASEME_DATA_DIR` for an isolated installation or test directory. The
Go event store uses an AES-256-GCM envelope and stores private files with
restrictive permissions. Credentials should be referenced through the canonical
`symvault://` form or a platform secure store.

## Migrating an older installation

The migration command is explicit and dry-run first:

```bash
symeraseme migrate \
  --source /path/to/old-state \
  --destination /path/to/go-state \
  --source-config /path/to/old-config \
  --destination-config /path/to/go-config \
  --dry-run
```

The source is never deleted. A complete backup is created before writes, and
`.migration-state.json` allows an interrupted migration to resume. Keep the
backup until the new CLI has been exercised successfully.

## Web forms and manual tasks

Web-form automation uses the `symaira-browse` integration. If a broker requires
an unsupported interaction, inspect the fallback queue:

```bash
symeraseme manual-tasks list
symeraseme manual-tasks show <task-id>
symeraseme manual-tasks complete <task-id>
```

Do not submit a real opt-out while debugging unless the user explicitly asked
for that action and the target data is appropriate for the operation.

## Email and triage

Inbox polling requires an IMAP account and a platform-appropriate credential.
Use a dry-run or isolated account while configuring it:

```bash
symeraseme poll-inbox --host imap.example.com --port 993 --username <address>
symeraseme classify-reply <request-id>
symeraseme generate-rebuttal <request-id>
```

LLM provider configuration is resolved by the shared Go provider layer. Missing
provider configuration is a setup error, not a reason to expose credentials in
an issue or log.

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

The GUI requires a full Xcode installation, not Command Line Tools alone. Build
from the repository root:

```bash
./app/SymairaEraseMe/build.sh
VERSION=0.12.0 ./scripts/package-dmg.sh
```

Release apps contain `Symaira EraseMe.app/Contents/MacOS/symeraseme`. A
local build may be ad-hoc signed; the release workflow records whether Developer
ID signing, notarization, and stapling were completed. An ad-hoc DMG is not
Gatekeeper-ready.

## Windows build

The Go CLI is CGO-free. A Windows cross-build can be checked without executing
it on macOS:

```bash
CGO_ENABLED=0 GOOS=windows GOARCH=amd64 go build ./cmd/symeraseme
CGO_ENABLED=0 GOOS=windows GOARCH=arm64 go build ./cmd/symeraseme
```

The event store uses the Windows `LockFileEx` API for non-blocking lock
semantics.

## Reporting a problem

Include the command, operating system, CLI version, and a redacted error. Do
not include profile files, token files, API keys, email contents, or personal
identity data. Security vulnerabilities belong in the repository's private
security reporting channel.
