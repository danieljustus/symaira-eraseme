# Symaira EraseMe Conventions

## Overview

Symaira EraseMe is a CLI tool for GDPR/CCPA data broker removal.

## Commands

### Setup
```bash
symeraseme init-profile        # Create encrypted identity
symeraseme show-profile        # Display identity
```

### Campaign Planning
```bash
symeraseme plan create --campaign initial --max 5
symeraseme plan show
symeraseme requests list
```

### Execution
```bash
symeraseme execute --campaign initial --batch-size 5 --dry-run  # Always dry-run first
symeraseme grant execute --ttl 3600                             # Issue consent token
symeraseme execute --campaign initial --batch-size 5 --consent <token>
```

### Triage
```bash
symeraseme poll-inbox --username user@gmail.com --since 3
symeraseme classify-reply <request_id>
symeraseme generate-rebuttal <request_id>
```

### Lifecycle
```bash
symeraseme tick --dry-run
symeraseme tick
```

## Best Practices

1. Always dry-run before destructive operations
2. Use `--output json` for machine-readable output
3. Start with small batch sizes (3-5)
4. Issue short-lived consent tokens
5. Run daily triage
6. Quarterly re-scans

## Environment

Set these variables:
- `ANTHROPIC_API_KEY` — For LLM triage
- `SYMERASEME_DATA_DIR` — Data directory
