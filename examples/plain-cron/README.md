# Scheduled removal cycle

This example runs the static `symeraseme` binary from cron. It assumes the
binary is on `PATH` and that the local profile/database were initialized.

## Install

```bash
brew install danieljustus/tap/symeraseme
symeraseme init-profile
```

## Environment

Keep provider and mail credentials in the platform secure store or use
`symvault://` references. Do not put resolved values in this file.

```bash
export SYMERASEME_DATA_DIR="$HOME/.local/share/symeraseme"
export SYMERASEME_LLM_PROVIDER="shared"
export IMAP_HOST="imap.example.com"
export IMAP_PORT="993"
export IMAP_USERNAME="user@example.com"
```

## Cron entries

Use the absolute path to the installed binary when cron does not load the
interactive shell `PATH`:

```cron
# Daily deadline tick at 09:00
0 9 * * * /usr/local/bin/symeraseme tick --output json >> "$HOME/.local/state/symeraseme/tick.log" 2>&1

# Daily inbox poll at 10:00
0 10 * * * /usr/local/bin/symeraseme poll-inbox --host "$IMAP_HOST" --port "$IMAP_PORT" --username "$IMAP_USERNAME" --output json >> "$HOME/.local/state/symeraseme/inbox.log" 2>&1
```

Generate native schedules for the current platform instead of maintaining cron
manually:

```bash
symeraseme generate-scheduler --platform cron --output ./schedules
symeraseme schedule status --platform cron
```

## First run

Always plan and inspect a campaign before any destructive execution:

```bash
CAMPAIGN="initial"
symeraseme plan create --campaign "$CAMPAIGN" --max 5 --output json
symeraseme plan show --campaign "$CAMPAIGN" --output json
symeraseme execute --campaign "$CAMPAIGN" --dry-run --output json
```

After explicit user consent, issue a short-lived token and execute the reviewed
plan:

```bash
symeraseme grant execute --ttl 3600
symeraseme execute --campaign "$CAMPAIGN" --consent-file /path/to/token
```

## Monitoring

```bash
symeraseme requests list --status pending --output json | jq .
symeraseme calendar --weeks 4 --output json | jq .
symeraseme generate-report --all-campaigns
```

The scheduler never replaces review of destructive actions. Keep logs local,
redact identity data before sharing diagnostics, and remove temporary test
schedules after verification.
