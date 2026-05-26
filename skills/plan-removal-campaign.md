# Plan a Removal Campaign

Guide an AI agent or user through planning a data broker removal campaign.

## Prerequisites

- [Identity profile created](setup-identity.md) (`symeraseme init-profile`)
- Database initialized (auto-initialized on first command)

## Overview

A campaign groups removal requests for a set of data brokers. Planning
scans the registry, filters brokers by criteria, and creates individual
removal requests in the event store.

## Step 1: Create a campaign

```bash
symeraseme plan create --campaign initial-2026-q2
```

The command scans the broker registry and plans requests for all known brokers.

### Filtering options

```bash
# Plan only GDPR (EU) brokers
symeraseme plan create --campaign gdpr-only --jurisdiction GDPR

# Plan only CCPA (US) brokers
symeraseme plan create --campaign ccpa-only --jurisdiction CCPA

# Plan by priority (high first)
symeraseme plan create --campaign high-priority --priority high

# Limit to 5 brokers
symeraseme plan create --campaign small-batch --max 5
```

### JSON output (for AI agents)

```bash
symeraseme plan create --campaign initial --output json
```

```json
{
  "campaign_id": "initial",
  "total_brokers": 32,
  "planned": 28,
  "requests": [
    {
      "request_id": 1,
      "broker_name": "Acxiom",
      "channel": "email",
      "jurisdiction": "GDPR"
    },
    {
      "request_id": 2,
      "broker_name": "Spokeo",
      "channel": "web_form",
      "jurisdiction": "CCPA"
    }
  ]
}
```

## Step 2: Review the plan with the user

```bash
symeraseme plan show
```

Filter by campaign or status:

```bash
symeraseme plan show --campaign initial
symeraseme plan show --status PLANNED
symeraseme plan show --status COMPLETED
```

### JSON output

```bash
symeraseme plan show --output json
```

```json
{
  "campaign_id": "initial",
  "total": 28,
  "requests": [
    {
      "id": 1,
      "broker_id": "acxiom",
      "current_status": "PLANNED"
    }
  ]
}
```

## Step 3: List removal requests

```bash
symeraseme requests list
symeraseme requests list --campaign initial
symeraseme requests list --status PENDING
symeraseme requests list --broker acxiom
```

## Best practices

1. **Always review with the user** before executing. Use `plan show` to display
   the plan and confirm the number of brokers and jurisdictions look correct.
2. **Start small**: Use `--max 5` for the first campaign to validate the setup.
3. **Filter by jurisdiction**: If the user is in the EU, use `--jurisdiction GDPR`
   to avoid sending CCPA-specific requests.
4. **Use descriptive campaign IDs**: Include the quarter and purpose, e.g.,
   `initial-2026-q2`, `quarterly-rescan-q3-2026`.

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `0 brokers planned` | No brokers match the filter | Remove `--jurisdiction` or `--priority` filters |
| `Campaign exists` error | Campaign ID already used | Pick a different campaign ID |
| No brokers found | Registry empty or not loaded | Check `registry/brokers/` directory exists |
