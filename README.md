# OpenEraseMe

**Automated data broker removal tool — close your accounts, erase your data.**

> **Alpha** — This project is in early development. APIs may change and some features are incomplete.

[![CI](https://img.shields.io/github/actions/workflow/status/danieljustus/OpenEraseMe/ci.yml?branch=main&label=CI&logo=github)](https://github.com/danieljustus/OpenEraseMe/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python](https://img.shields.io/badge/python-3.11%2B-blue)](https://python.org)

OpenEraseMe helps you exercise your GDPR/CCPA right to erasure against
data brokers. It provides:

- **A curated registry** of 30+ data brokers with opt-out processes documented
- **CLI tools** to plan, send, and track removal requests
- **Skills** for LLM-powered agents (Claude Code, OpenClaw, etc.)
- **Lifecycle management** with deadline tracking, reminders, and escalation

## Features

- **Curated broker registry** with YAML-based definitions for 30+ data brokers, including opt-out URLs, required account identifiers, contact methods (web forms, email, API), and escalation paths for non-compliance.
- **CLI automation** to plan removal campaigns, send opt-out requests in batches, track progress per broker, and monitor results over time from the terminal.
- **Deadline tracking** with automatic 30-day GDPR deadline monitoring and configurable reminders. Escalation workflows trigger when brokers miss the legal response window.
- **LLM agent skills** as ready-made skill files for Claude Code, OpenClaw, and other LLM-powered coding agents. These skills let AI assistants work with the tool on your behalf.
- **Jurisdiction-aware workflows** with support for GDPR (Europe) and CCPA (California) erasure rights, including jurisdiction-specific templates, timelines, and legal references.

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
openeraseme brokers show --name AcmeDataCorp
```

### Demo

```console
$ openeraseme init-profile
✓ Profile saved to ~/.config/openeraseme/profile.json

$ openeraseme brokers list --jurisdiction GDPR
┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━┓
┃ Name         ┃ Website                     ┃ Jurisdiction  ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━┩
│ AcmeDataCorp │ https://acmedata.example    │ GDPR, CCPA    │
│ BrokerB      │ https://brokerb.example     │ GDPR          │
│ DataVault    │ https://datavault.example   │ CCPA          │
└──────────────┴─────────────────────────────┴───────────────┘

$ openeraseme plan --jurisdiction GDPR --max 3
✓ Plan created: 3 brokers selected
  Campaign: initial
  Output: ~/.config/openeraseme/campaigns/initial/plan.json

$ openeraseme status
Campaign: initial
┏━━━━━━━━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━┓
┃ Broker       ┃ Status   ┃ Deadline             ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━┩
│ AcmeDataCorp │ pending  │ 2026-06-20 (30 days) │
│ BrokerB      │ pending  │ 2026-06-20 (30 days) │
│ DataVault    │ sent     │ 2026-06-20 (30 days) │
└──────────────┴──────────┴──────────────────────┘
```

### Planning and execution

```bash
# Create a removal plan for GDPR brokers (limit to 10)
openeraseme plan --jurisdiction GDPR --max 10

# Execute the plan in batches (respects rate limits)
openeraseme execute --campaign initial --batch-size 5 --delay 30

# Check overall campaign progress
openeraseme status

# View deadline calendar and upcoming reminders
openeraseme calendar --weeks 4
```

### Other commands

```bash
# Validate registry YAML files against the schema
openeraseme validate

# Export campaign data for record-keeping
openeraseme export --format json --output campaign.json
```

Run `openeraseme --help` for a full list of commands and options.

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

The codebase uses a `src` layout under `src/openeraseme/`, with broker definitions in YAML under `registry/brokers/` and their JSON schema in `registry/schemas/`.

## Documentation

- [Architecture Plan](docs/architektur-plan-v0.1.md) — Design overview and data flow
- [Contributing Guide](CONTRIBUTING.md) — How to add brokers, submit changes, and report issues

## License

MIT
