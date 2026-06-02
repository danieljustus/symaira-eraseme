# Symaira EraseMe Guidelines

## Overview

Symaira EraseMe is a CLI tool for GDPR/CCPA data broker removal. It provides:
- 1,200+ curated data broker definitions
- Campaign planning and execution
- Inbox triage with LLM classification
- Deadline tracking and escalation
- Quarterly re-scan workflows

## When to Use

Use Symaira EraseMe when the user wants to:
- Remove personal data from data broker databases
- Exercise GDPR/CCPA right to erasure
- Plan and execute opt-out campaigns
- Track removal request status
- Respond to broker replies

## CLI Commands

### Setup
- `symeraseme init-profile` — Create encrypted identity
- `symeraseme show-profile` — Display identity
- `symeraseme db-init` — Initialize database

### Planning
- `symeraseme plan create --campaign <id>` — Create campaign
- `symeraseme plan show` — Review plan
- `symeraseme requests list` — List requests

### Execution
- `symeraseme execute --campaign <id> --batch-size 5` — Send requests
- `symeraseme grant execute --ttl 3600` — Issue consent token

### Triage
- `symeraseme poll-inbox --username <email>` — Fetch replies
- `symeraseme classify-reply <request_id>` — Classify reply
- `symeraseme generate-rebuttal <request_id>` — Generate rebuttal

### Lifecycle
- `symeraseme tick` — Run deadline engine
- `symeraseme calendar --weeks 4` — View deadline calendar

## Best Practices

1. Always use `--dry-run` before destructive operations
2. Start with `--batch-size 3` for rate limiting
3. Use `--output json` for structured data
4. Review plans with user before execution
5. Run daily triage: `poll-inbox` + `classify-reply`
6. Quarterly re-scans with new campaign IDs

## Environment Variables

- `ANTHROPIC_API_KEY` — Required for LLM triage
- `SYMERASEME_DATA_DIR` — Data directory path
