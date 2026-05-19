# OpenEraseMe

**Automated data broker removal tool — close your accounts, erase your data.**

OpenEraseMe helps you exercise your GDPR/CCPA right to erasure against
data brokers. It provides:

- **A curated registry** of 30+ data brokers with opt-out processes documented
- **CLI tools** to plan, send, and track removal requests
- **Skills** for LLM-powered agents (Claude Code, OpenClaw, etc.)
- **Lifecycle management** with deadline tracking, reminders, and escalation

## Quick Start

```bash
uv sync
uv pip install -e .
openeraseme init-profile
openeraseme brokers list --jurisdiction GDPR
openeraseme plan --jurisdiction GDPR --max 10
openeraseme execute --campaign initial --batch-size 5
```

## Documentation

- [Architecture Plan](docs/architektur-plan-v0.1.md)
- [Contributing Guide](CONTRIBUTING.md)

## License

MIT
