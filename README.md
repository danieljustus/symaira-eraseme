# OpenEraseMe

**Automated data broker removal tool — close your accounts, erase your data.**

> **Beta** — Core features are stable and tested. Some advanced features (web-form CAPTCHA solving, DPA auto-filing) require manual setup or are event-flagged only.

[![CI](https://img.shields.io/github/actions/workflow/status/danieljustus/OpenEraseMe/ci.yml?branch=main&label=CI&logo=github)](https://github.com/danieljustus/OpenEraseMe/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python](https://img.shields.io/badge/python-3.11%2B-blue)](https://python.org)

OpenEraseMe helps you exercise your GDPR/CCPA right to erasure against
data brokers. It provides:

- **A curated registry** of 600+ data brokers with opt-out processes documented
- **CLI tools** to plan, send, track, and triage removal requests
- **Skills** for LLM-powered agents (Claude Code, OpenClaw, etc.)
- **Lifecycle management** with deadline tracking, reminders, escalation, and re-scans
- **Automated registry maintenance** via weekly scans of public state broker registries

## Features

- **Curated broker registry** with YAML-based definitions for **602 data brokers** across the EU (37), UK (6), and US (559), including opt-out URLs, required account identifiers, contact methods (web forms, email), and verification keywords.
- **Event-sourced architecture** with an append-only SQLite event store, state projections, and full audit trail for every removal request.
- **CLI automation** with 30+ commands to plan removal campaigns, send opt-out requests in batches, track progress, monitor deadlines, and triage broker replies from the terminal.
- **Web-form automation** via Playwright for brokers that only accept opt-outs through web forms, including form-filling, CAPTCHA detection, and screenshot capture.
- **Inbox triage** via IMAP polling to fetch broker replies, classify them with an LLM (Claude), and generate jurisdiction-aware rebuttals for rejections.
- **Deadline tracking** with automatic jurisdiction-aware deadline monitoring (GDPR: 30 days, CCPA: 45 days). The tick engine checks daily for overdue requests and triggers reminders with exponential backoff.
- **Escalation workflows** that flag requests for DPA complaints after brokers miss the legal response window.
- **LLM agent skills** as ready-made skill files for Claude Code, OpenClaw, and other LLM-powered coding agents. These skills let AI assistants work with the tool on your behalf.
- **Jurisdiction-aware workflows** with support for GDPR (Europe), CCPA (California), CPRA, LGPD, and PIPEDA erasure rights, including jurisdiction-specific templates, timelines, and legal references.
- **Scheduler integration** that generates cron, launchd, or systemd configurations to run the tick engine, inbox polling, and quarterly re-scans automatically.
- **Automated registry maintenance** with a weekly GitHub Action that pulls fresh entries from official US state broker registries and opens a PR with the diff, plus a Monday link-check workflow that flags dead broker websites.
- **Dashboard and reports** for campaign analytics, jurisdiction breakdowns, and GDPR-compliant record-keeping exports.

## Install

**End users** (from PyPI):

```bash
pip install openeraseme
```

Optional extras:

```bash
pip install openeraseme[web]      # Playwright-based browser automation
pip install openeraseme[triage]   # LLM triage via Anthropic Claude
```

**Developers** (from source):

```bash
# Clone the repository
git clone https://github.com/danieljustus/OpenEraseMe.git
cd OpenEraseMe

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
openeraseme init-profile

# List all registered brokers, optionally filtered by jurisdiction
openeraseme brokers list --jurisdiction GDPR

# Show details for a specific broker
openeraseme brokers show --name spokeo
```

### Demo

```console
$ openeraseme init-profile
✓ Profile saved to ~/.config/openeraseme/profile.json

$ openeraseme brokers list --jurisdiction GDPR
┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━┓
┃ Name         ┃ Website                     ┃ Jurisdiction  ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━┩
│ Spokeo       │ https://www.spokeo.com      │ CCPA          │
│ Intelius     │ https://www.intelius.com    │ CCPA          │
│ Acxiom (EU)  │ https://www.acxiom.com      │ GDPR          │
│ Schufa       │ https://www.schufa.de       │ GDPR          │
└──────────────┴─────────────────────────────┴───────────────┘

$ openeraseme plan create --campaign initial --jurisdiction GDPR --max 5
✓ Plan created: 5 brokers selected
  Campaign: initial

$ openeraseme status
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
openeraseme plan create --campaign initial --jurisdiction GDPR --max 10

# Review the plan before sending
openeraseme plan show --campaign initial

# Execute the plan in batches (respects rate limits, requires consent)
openeraseme execute --campaign initial --batch-size 5 --delay 30 --yes

# Check overall campaign progress
openeraseme status

# View deadline calendar and upcoming tick actions
openeraseme calendar --weeks 4
```

### Inbox triage (requires `[triage]` extra)

```bash
# Poll your IMAP inbox for broker replies
openeraseme poll-inbox --username your@email.com

# Classify a broker reply via LLM
openeraseme classify-reply <request_id>

# Generate a jurisdiction-aware rebuttal for a rejection
openeraseme generate-rebuttal <request_id>
```

### Web-form automation (requires `[web]` extra)

```bash
# Run a broker's web-form opt-out via Playwright
openeraseme run-web-form <broker_id>

# List manual fallback tasks for forms that couldn't be automated
openeraseme manual-tasks list

# Mark a manual task as completed
openeraseme manual-tasks complete <task_id>
```

### Lifecycle and maintenance

```bash
# Run the tick engine (checks deadlines, reminders, escalations)
openeraseme tick --dry-run
openeraseme tick

# Generate scheduler configs (cron / launchd / systemd)
openeraseme generate-scheduler --output ./schedules

# Install schedules
openeraseme schedule install ./schedules

# Generate a dashboard report
openeraseme generate-dashboard

# Export campaign data for GDPR record-keeping
openeraseme export --format json --output campaign.json
```

### Other commands

```bash
# Validate registry YAML files against the schema
openeraseme validate

# Show event history for a request
openeraseme events show <request_id>

# List all removal requests
openeraseme requests list --status pending

# Grant consent for destructive operations
openeraseme grant execute --ttl 3600
```

Run `openeraseme --help` for a full list of commands and options.

## Architecture

OpenEraseMe uses an **event-sourced architecture** built on SQLite:

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

### Run tests

```bash
uv run pytest --verbose --tb=short
```

### Lint and type-check

```bash
uv run ruff check src/openeraseme/
uv run ruff format --check src/openeraseme/
uv run mypy src/openeraseme/
```

All three checks run in CI on every push and pull request to the `main` branch.

### Project structure

```
src/openeraseme/
  cli/           — Typer CLI application
  core/          — Event store, projections, tick engine, templating, scheduler
  registry/      — Broker loader, schema validation
  services/      — CLI command handlers
  adapters/      — Web (Playwright), Triage (Claude), Email (SMTP/IMAP)
registry/
  brokers/       — YAML broker definitions (eu/, uk/, us/)
  laws/          — Jinja2 legal templates (GDPR, CCPA, rebuttals)
  schemas/       — JSON Schema for broker validation
skills/          — LLM agent skill files (Claude Code, OpenClaw)
examples/        — Integration examples for Claude Code, OpenClaw, cron
```

## License

MIT
