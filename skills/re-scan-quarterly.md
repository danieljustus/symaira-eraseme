# Quarterly Re-Scan

Guide an AI agent or user through the quarterly re-scan workflow to detect
re-added data and plan new removal campaigns.

## Prerequisites

- Existing campaigns completed or at least 90 days old
- [Tick engine](daily-tick.md) has marked some requests as `COMPLETE`

## Why re-scan quarterly

Data brokers may re-add your information after the initial opt-out period
(typically 30-90 days). Quarterly re-scans ensure your data stays removed.

## Step 1: Review completed requests

First, identify which requests are due for re-scan:

```bash
symeraseme requests list --status COMPLETED
symeraseme requests list --status OVERDUE
```

## Step 2: Plan a new campaign

Create a fresh campaign for the new quarter:

```bash
symeraseme plan create --campaign q3-2026-rescan
```

This scans all brokers in the registry — including any new brokers added
since the last plan.

### Filtering for re-scan

```bash
# If only GDPR brokers need re-scanning
symeraseme plan create --campaign q3-2026-rescan --jurisdiction GDPR

# If a specific broker re-added data
symeraseme plan create --campaign rescan-acxiom --max 1
```

## Step 3: Review and execute

```bash
# Review the plan
symeraseme plan show --campaign q3-2026-rescan

# Execute (after consent)
symeraseme execute --campaign q3-2026-rescan --batch-size 5
```

## Step 4: Compare with previous campaigns

```bash
# View previous campaign results
symeraseme requests list --campaign initial-2026-q1
symeraseme requests list --campaign initial-2026-q2
```

## Complete quarterly workflow

```bash
# 1. Plan the re-scan
symeraseme plan create --campaign q3-2026-rescan

# 2. Review with user
symeraseme plan show --campaign q3-2026-rescan

# 3. Execute (after consent)
symeraseme grant execute --ttl 7200
symeraseme execute --campaign q3-2026-rescan --batch-size 5 --consent <token>

# 4. Set up daily triage
# Remind user to run poll-inbox + classify-reply daily

# 5. Run initial tick
symeraseme tick
```

## Best practices

1. **Set calendar reminders**: Schedule quarterly re-scans at the start of
   each quarter (Jan, Apr, Jul, Oct).
2. **Check for new brokers**: The registry may have been updated with new
   brokers since your last scan.
3. **Review broker replies**: Some brokers may have changed their opt-out
   process since last time.
4. **Document results**: Keep track of which brokers re-added data for
   potential escalation.

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| All requests already completed | Re-scan not needed yet | Check broker response times |
| No new brokers planned | Registry unchanged from last scan | That's fine — run anyway for re-added data |
| Existing campaign data lost | Database reset or migration | Check SQLite database at default path |
