# Symaira EraseMe

**Automated data broker removal tool — close your accounts, erase your data.**

> **Beta** — Core features are stable and tested. Some advanced features (web-form CAPTCHA solving, DPA auto-filing) require manual setup or are event-flagged only.

[![CI](https://img.shields.io/github/actions/workflow/status/danieljustus/Symaira-EraseMe/ci.yml?branch=main&label=CI&logo=github)](https://github.com/danieljustus/Symaira-EraseMe/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python](https://img.shields.io/badge/python-3.11%2B-blue)](https://python.org)

<p align="center">
  <img src="https://raw.githubusercontent.com/danieljustus/symaira-eraseme/main/assets/terminal-demo.svg" alt="Symaira EraseMe terminal demo — init-profile, broker listing, plan creation, and campaign status" width="760">
</p>

Symaira EraseMe helps you exercise your GDPR/CCPA right to erasure against
data brokers. It provides:

- **A curated registry** of 1,200+ data brokers with opt-out processes documented
- **CLI tools** to plan, send, track, and triage removal requests
- **Skills** for LLM-powered agents (Claude Code, OpenClaw, etc.)
- **Lifecycle management** with deadline tracking, reminders, escalation, and re-scans
- **Automated registry maintenance** via weekly scans of public state broker registries

## Features

- **Curated broker registry** with YAML-based definitions for **1,277 data brokers** across the EU (121), UK (20), and US (1,138), including opt-out URLs, required account identifiers, contact methods (web forms, email), and verification keywords.
- **Event-sourced architecture** with an append-only SQLite event store, state projections, and full audit trail for every removal request.
- **CLI automation** with 30+ commands to plan removal campaigns, send opt-out requests in batches, track progress, monitor deadlines, and triage broker replies from the terminal.
- **Web-form automation** via Playwright for brokers that only accept opt-outs through web forms, including form-filling, CAPTCHA detection, and screenshot capture.
- **Inbox triage** via IMAP polling to fetch broker replies, classify them with an LLM (Claude), and generate jurisdiction-aware rebuttals for rejections.
- **Deadline tracking** with automatic jurisdiction-aware deadline monitoring (GDPR: 30 days, CCPA: 45 days). The tick engine checks daily for overdue requests and triggers reminders with exponential backoff.
- **Escalation workflows** that flag requests for DPA complaints after brokers miss the legal response window.
- **LLM agent skills** as ready-made skill files for Claude Code, Cursor, Windsurf, Hermes, GitHub Copilot, Codex, and other LLM-powered coding agents. These skills let AI assistants work with the tool on your behalf. See [AGENTS.md](AGENTS.md) for setup instructions.
- **Jurisdiction-aware workflows** with support for GDPR (Europe), CCPA (California), CPRA, LGPD, and PIPEDA erasure rights, including jurisdiction-specific templates, timelines, and legal references.
- **Scheduler integration** that generates cron, launchd, or systemd configurations to run the tick engine, inbox polling, and quarterly re-scans automatically.
- **Automated registry maintenance** with a weekly GitHub Action that pulls fresh entries from official US state broker registries and opens a PR with the diff, plus a Monday link-check workflow that flags dead broker websites.
- **Dashboard and reports** for campaign analytics, jurisdiction breakdowns, and GDPR-compliant record-keeping exports.

## Install

**End users** (from PyPI):

```bash
pip install symeraseme
```

Optional extras:

```bash
pip install symeraseme[web]      # Playwright-based browser automation
pip install symeraseme[triage]   # LLM triage via Anthropic Claude
```

**macOS users** (via Homebrew):

```bash
brew tap danieljustus/symaira
brew install symeraseme
```

**Developers** (from source):

```bash
# Clone the repository
git clone https://github.com/danieljustus/Symaira-EraseMe.git
cd symaira-eraseme

# Install dependencies with uv
uv sync
uv pip install -e ".[dev,web,triage]"

# Configure your environment
cp .env.example .env
# Edit .env and add your ANTHROPIC_API_KEY
```

See `.env.example` for all supported environment variables.

## Usage

### Getting started

```bash
# Initialize your profile with personal details
symeraseme init-profile

# List all registered brokers, optionally filtered by jurisdiction or law
symeraseme brokers list --law GDPR

# Show details for a specific broker
symeraseme brokers show --name spokeo
```

### Demo

```console
$ symeraseme init-profile
✓ Profile saved to ~/.config/symeraseme/profile.json

$ symeraseme brokers list --law GDPR
┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━┓
┃ Name         ┃ Website                     ┃ Jurisdiction  ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━┩
│ Spokeo       │ https://www.spokeo.com      │ CCPA          │
│ Intelius     │ https://www.intelius.com    │ CCPA          │
│ Acxiom (EU)  │ https://www.acxiom.com      │ GDPR          │
│ Schufa       │ https://www.schufa.de       │ GDPR          │
└──────────────┴─────────────────────────────┴───────────────┘

$ symeraseme plan create --campaign initial --law GDPR --max 5
✓ Plan created: 5 brokers selected
  Campaign: initial

$ symeraseme plan status
Campaign: initial
┏━━━━━━━━━━━━━┳━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━┓
┃ Broker      ┃ Status      ┃ Deadline             ┃
┡━━━━━━━━━━━━━╇━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━┩
│ Acxiom (EU) │ planned     │ —                    │
│ Schufa      │ planned     │ —                    │
│ Experian EU │ planned     │ —                    │
│ Equifax EU  │ planned     │ —                    │
│ Creditreform│ planned     │ —                    │
└─────────────┴─────────────┴──────────────────────┘
```

### Planning and execution

```bash
# Create a removal plan for GDPR brokers (limit to 10)
symeraseme plan create --campaign initial --jurisdiction GDPR --max 10

# Review the plan before sending
symeraseme plan show --campaign initial

# Execute the plan in batches (respects rate limits, requires consent)
symeraseme plan execute --campaign initial --batch-size 5 --delay 30 --yes

# Check overall campaign progress
symeraseme plan status

# View deadline calendar and upcoming tick actions
symeraseme calendar --weeks 4
```

### Inbox triage (requires `[triage]` extra)

```bash
# Poll your IMAP inbox for broker replies
symeraseme poll-inbox --username your@email.com

# Classify a broker reply via LLM
symeraseme classify-reply <request_id>

# Generate a jurisdiction-aware rebuttal for a rejection
symeraseme generate-rebuttal <request_id>
```

### Web-form automation (requires `[web]` extra)

```bash
# Run a broker's web-form opt-out via Playwright
symeraseme run-web-form <broker_id>

# List manual fallback tasks for forms that couldn't be automated
symeraseme manual-tasks list

# Mark a manual task as completed
symeraseme manual-tasks complete <task_id>
```

### Lifecycle and maintenance

```bash
# Run the tick engine (checks deadlines, reminders, escalations)
symeraseme plan tick --dry-run
symeraseme plan tick

# Generate scheduler configs (cron / launchd / systemd)
symeraseme generate-scheduler --output ./schedules

# Install schedules
symeraseme schedule install ./schedules

# Generate a dashboard report
symeraseme generate-dashboard

# Export campaign data for GDPR record-keeping
symeraseme export --format json --output campaign.json
```

### Other commands

```bash
# Validate registry YAML files against the schema
symeraseme validate

# Show event history for a request
symeraseme events show <request_id>

# List all removal requests
symeraseme requests list --status pending

# Grant consent for destructive operations
symeraseme grant execute --ttl 3600
```

Run `symeraseme --help` for a full list of commands and options.

## Architecture

Symaira EraseMe uses an **event-sourced architecture** built on SQLite:

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│   CLI /     │────▶│   Event      │────▶│  Request    │
│   Skills    │     │   Store      │     │  State      │
└─────────────┘     │  (SQLite)    │     │ (Projection)│
                    └──────────────┘     └─────────────┘
                           │
                    ┌──────┴──────┐
                    ▼             ▼
            ┌──────────┐   ┌──────────┐
            │  Tick    │   │  Reports │
            │  Engine  │   │ / Export │
            └──────────┘   └──────────┘
```

- **Event Store**: Append-only log of all actions (planned, sent, ack, reminder, deadline reached, etc.)
- **State Projection**: Rebuilds the current state of every request from events
- **Tick Engine**: Daily scan for deadlines, reminders, and escalations
- **Triage**: LLM-based classification of broker replies with jurisdiction-aware rebuttal generation

## Registry maintenance

The broker registry is kept fresh by two scheduled GitHub Actions:

- **`registry-scanner`** (Sundays, 00:00 UTC) — fetches the latest data-broker
  registries published by US states (e.g. California, Vermont, Oregon, Texas),
  normalizes the records into YAML entries, and opens a pull request for any
  additions or changes.
- **`registry-link-check`** (Mondays, 06:00 UTC) — issues a `HEAD` request to
  every broker's `website` field and reports unreachable URLs so dead entries
  can be retired or corrected.

You can also run the sync manually:

```bash
uv run python scripts/registry_sync.py
```

## Development

### Setup

```bash
uv sync --all-extras
uv pip install -e ".[dev,web,triage]"
pre-commit install
```

The pre-commit hooks include:
- **Detect private keys** — checks for hardcoded private keys

CI additionally runs **TruffleHog** for comprehensive secrets scanning on every pull request.

### Run tests

```bash
uv run pytest --verbose --tb=short
```

### Lint and type-check

```bash
uv run ruff check src/symeraseme/
uv run ruff format --check src/symeraseme/
uv run mypy src/symeraseme/
```

All three checks run in CI on every push and pull request to the `main` branch.

### Project structure

```
src/symeraseme/
  cli/           — Typer CLI application
  core/          — Event store, projections, tick engine, templating, scheduler
  registry/      — Broker loader, schema validation
  services/      — CLI command handlers
  adapters/      — Web (Playwright), Triage (Claude), Email (SMTP/IMAP)
registry/
  brokers/       — YAML broker definitions (eu/, uk/, us/)
  laws/          — Jinja2 legal templates (GDPR, CCPA, rebuttals)
  schemas/       — JSON Schema for broker validation
skills/          — LLM agent skill files (Claude Code, Cursor, Windsurf, Hermes, Copilot, Codex)
examples/        — Integration examples for all supported AI agents
AGENTS.md        — Setup guide for all AI agent integrations
```

## Security

- **Identity profile encryption**: Profiles are encrypted with AES-256-GCM and authenticated with the header as AAD. Files written since v0.1.2 use header `version: 2`; earlier files used `version: 1`. A legacy no-AAD fallback exists for `version: 0` files only — any tampered ciphertext on version 1+ fails closed with `InvalidTag`.
- **Database encryption**: When `SYMERASEME_ENCRYPT_DB=1` is set, the SQLite database is encrypted at rest using AES-256-GCM with a key derived from your identity master key. Databases created before v0.2.0 used a fixed PBKDF2 salt (V1 format); newer databases use a per-file random salt (V2 format). On open, any V1 database is automatically re-encrypted to V2 transparently — no manual migration is required. On open, the database is decrypted to a temporary file with restrictive permissions (`0o600`). The temp file is placed in a secure temporary directory. On Linux `/dev/shm` (tmpfs, memory-backed) is used when available. On macOS and Windows the OS temp directory is used, which may be disk-backed. On normal exit, SIGTERM, or context close, the temp file is re-encrypted and removed. A startup scavenger removes any stale temp files older than 5 minutes from previous aborted runs. A `SIGKILL` (e.g., `kill -9`, OOM killer, or system crash) may leave the decrypted temp file behind temporarily; the 5-minute scavenger window limits exposure. If this is a concern for your threat model, consider running Symaira EraseMe on a single-user system, using full-disk encryption, or setting `TMPDIR` to a RAM disk (e.g., `/dev/shm` on Linux).
- **Consent tokens**: Consent tokens passed via `--consent` or the `SYMERASEME_CONSENT` environment variable are visible in process listings (`ps aux`), shell history, and crash dumps. On shared systems or CI runners, prefer `--consent-file` or `SYMERASEME_CONSENT_FILE` to read the token from a file with `0o600` permissions. The file is read once and the token is consumed (`consume_token`) after verification. Pipe-based input is supported: `echo $TOKEN | symeraseme plan execute --consent-file /dev/stdin`.

## License

MIT
