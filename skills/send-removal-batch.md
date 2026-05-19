# Send Removal Requests

Guide an AI agent or user through executing a planned removal campaign.

## Prerequisites

- [Campaign planned](plan-removal-campaign.md) (`openeraseme plan create`)
- [Email accounts configured](../SKILL.md) (`openeraseme accounts add`)
- Consent token issued for destructive operations

## Step 1: Dry-run first

Always validate the plan before sending real requests:

```bash
openeraseme execute --campaign initial --dry-run
```

This simulates sending without actually dispatching emails or submitting forms.

### JSON output

```bash
openeraseme execute --campaign initial --dry-run --output json
```

```json
{
  "results": [
    {
      "request_id": 1,
      "success": true,
      "dry_run": true
    },
    {
      "request_id": 2,
      "success": true,
      "dry_run": true
    }
  ]
}
```

## Step 2: Obtain consent

Destructive commands require explicit consent. Choose one:

```bash
# Option A: Interactive prompt (run without flags)
openeraseme execute --campaign initial --batch-size 5
# You will be prompted: "This is a destructive operation. Continue? [y/N]"

# Option B: Pre-issue a consent token (for automation)
openeraseme grant execute --ttl 3600
# Output: Consent token: <token>
openeraseme execute --campaign initial --consent <token>

# Option C: Skip consent with --yes (interactive only)
openeraseme execute --campaign initial --yes
```

## Step 3: Execute the campaign

```bash
openeraseme execute --campaign initial --batch-size 5
```

### Batch size recommendations

| Batch size | Use case |
|------------|----------|
| 1-3 | First run, testing setup |
| 5-10 | Normal daily operation |
| 10-20 | Catch-up after extended pause |

## Consent tokens

Consent tokens allow automated execution without interactive prompts:

```bash
# Issue a token valid for 1 hour
openeraseme grant execute --ttl 3600

# List active tokens
openeraseme grant --list

# Revoke a specific token
openeraseme grant --revoke <token>

# Revoke all active tokens
openeraseme grant --revoke-all
```

### JSON output

```bash
openeraseme grant --list --output json
```

```json
[
  {
    "token": "abc123...",
    "command": "execute",
    "expires_at": "2026-05-20T15:00:00"
  }
]
```

## Best practices

1. **Always dry-run first**: Use `--dry-run` to verify the plan before sending.
2. **Drip sending**: Start with `--batch-size 3` to avoid rate limits. Increase
   gradually as brokers respond.
3. **Short-lived tokens**: Issue tokens with short TTLs (e.g., 3600s = 1 hour)
   for automation scripts.
4. **Review after execution**: Use `openeraseme requests list` to check
   which requests were sent successfully.
5. **Monitor responses**: Run `poll-inbox` after a few days to catch broker
   replies (see [triage workflow](triage-broker-replies.md)).

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `Error: Destructive command requires consent` | No consent given | Use `--yes` or issue a token via `grant` |
| `Campaign 'X' not found` | Campaign not planned | Run `plan create` first |
| `Error sending email` | Email account not configured | Run `accounts add <provider>` |
| `No removal requests planned` | No matching requests | Check `plan show` for the campaign |
