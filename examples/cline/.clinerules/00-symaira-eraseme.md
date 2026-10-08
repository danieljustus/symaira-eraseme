---
description: Symaira EraseMe data broker removal tool
alwaysApply: false
---

# Symaira EraseMe

## Overview

Symaira EraseMe is a CLI tool for GDPR/CCPA data broker removal with:
- 1,200+ curated broker definitions
- Campaign planning and execution
- LLM-powered inbox triage
- Deadline tracking and escalation
- Quarterly re-scan workflows

## When to Use

Activate this rule when the user wants to:
- Remove personal data from data brokers
- Exercise GDPR/CCPA erasure rights
- Plan opt-out campaigns
- Track removal request status
- Handle broker replies

## Workflow

1. **Setup**: `symeraseme init-profile`
2. **Plan**: `symeraseme plan create --campaign initial --max 5`
3. **Review**: `symeraseme plan show`
4. **Execute**: `symeraseme plan execute --campaign initial --batch-size 5 --consent <token>`
5. **Triage**: `symeraseme poll-inbox && symeraseme classify-reply`
6. **Tick**: `symeraseme tick`
7. **Re-scan**: Quarterly with new campaign IDs

## Key Commands

- `symeraseme init-profile` — Create identity
- `symeraseme plan create --campaign <id>` — Plan campaign
- `symeraseme plan execute --campaign <id> --consent <token>` — Run the plan; the Rust-only candidate has no outbound email/browser adapters by default
- `symeraseme tick` — Check deadlines
- `symeraseme poll-inbox` — Fetch replies
- `symeraseme classify-reply <id>` — Classify with LLM
- `symeraseme generate-rebuttal <id>` — Generate legal rebuttal

## Best Practices

- Always `--dry-run` first
- Use `--output json` for structured data
- Start with `--batch-size 3`
- Issue consent tokens with short TTL
- Daily triage routine

## Environment

`ANTHROPIC_API_KEY` is required only for LLM calls using Anthropic.
`SYMERASEME_DATA_DIR` is an optional data-directory override.
