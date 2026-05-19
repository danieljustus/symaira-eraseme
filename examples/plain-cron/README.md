# Cron-only Integration

This example shows how to run OpenEraseMe maintenance tasks on a schedule
using cron, without any AI agent.

## Prerequisites

- OpenEraseMe installed (`uv sync`)
- Email account configured for IMAP polling
- Consent token issued for automated execution
- `ANTHROPIC_API_KEY` set (for triage)

## Crontab entries

Add these entries to your crontab (`crontab -e`):

```cron
# ┌───────────── minute (0-59)
# │ ┌───────────── hour (0-23)
# │ │ ┌───────────── day of month (1-31)
# │ │ │ ┌───────────── month (1-12)
# │ │ │ │ ┌───────────── day of week (0-7, 0=Sun)
# │ │ │ │ │
# * * * * * command_to_execute

# ── Daily maintenance ──────────────────────────────────
# Poll inbox at 08:00 and 18:00 daily
0 8,18 * * * cd /home/jane/openeraseme && ./cron/poll-inbox.sh >> ./cron/logs/poll.log 2>&1

# Run tick engine at 08:30 daily
30 8 * * * cd /home/jane/openeraseme && ./cron/tick.sh >> ./cron/logs/tick.log 2>&1

# ── Weekly maintenance ─────────────────────────────────
# Classify unmatched replies every Monday at 09:00
0 9 * * 1 cd /home/jane/openeraseme && ./cron/classify-replies.sh >> ./cron/logs/classify.log 2>&1

# ── Quarterly maintenance ──────────────────────────────
# Re-scan on the first day of each quarter at 10:00
0 10 1 1,4,7,10 * cd /home/jane/openeraseme && ./cron/quarterly-rescan.sh >> ./cron/logs/rescan.log 2>&1
```

## Helper scripts

### `cron/poll-inbox.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# Source environment
source ./cron/env.sh

echo "[$(date)] Starting inbox poll..."

uv run openeraseme poll-inbox \
  --host "$IMAP_HOST" \
  --port "$IMAP_PORT" \
  --username "$IMAP_USERNAME" \
  --password "$IMAP_PASSWORD" \
  --since 3 \
  --output json 2>&1

echo "[$(date)] Inbox poll complete."
```

### `cron/tick.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./cron/env.sh

echo "[$(date)] Running tick engine..."

uv run openeraseme tick --output json 2>&1

echo "[$(date)] Tick complete."
```

### `cron/classify-replies.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./cron/env.sh

echo "[$(date)] Classifying unmatched replies..."

# Get unmatched requests
uv run openeraseme requests list --status PENDING --output json | \
  python3 -c "
import json, sys, subprocess
data = json.load(sys.stdin)
for req in data:
    rid = req.get('id')
    if rid:
        print(f'Classifying request #{rid}...')
        result = subprocess.run(
            ['uv', 'run', 'openeraseme', 'classify-reply', str(rid), '--api-key', '$ANTHROPIC_API_KEY'],
            capture_output=True, text=True
        )
        print(result.stdout)
        if result.stderr:
            print(f'  Error: {result.stderr}', file=sys.stderr)
"

echo "[$(date)] Classification complete."
```

### `cron/quarterly-rescan.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./cron/env.sh

QUARTER=$(date +%Y-q$(( ($(date +%-m)-1)/3+1 )))
CAMPAIGN="rescan-$QUARTER"

echo "[$(date)] Starting quarterly re-scan ($CAMPAIGN)..."

# Plan the campaign
uv run openeraseme plan create --campaign "$CAMPAIGN" --output json 2>&1

# Dry-run first
uv run openeraseme execute --campaign "$CAMPAIGN" --dry-run --output json 2>&1

# Execute (requires valid consent token in env)
if [ -n "${CONSENT_TOKEN:-}" ]; then
  uv run openeraseme execute \
    --campaign "$CAMPAIGN" \
    --batch-size 5 \
    --consent "$CONSENT_TOKEN" \
    --output json 2>&1
else
  echo "  SKIP: No CONSENT_TOKEN set — execution requires manual consent."
fi

echo "[$(date)] Quarterly re-scan complete."
```

### `cron/env.sh`

```bash
#!/usr/bin/env bash
# ── Cron environment for OpenEraseMe ────────────────────
# Source this file from all cron helper scripts.

# OpenEraseMe paths
export OPENERASEME_DATA_DIR="${OPENERASEME_DATA_DIR:-$HOME/.openeraseme}"
export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:$PATH"

# IMAP settings
export IMAP_HOST="${IMAP_HOST:-imap.gmail.com}"
export IMAP_PORT="${IMAP_PORT:-993}"
export IMAP_USERNAME="${IMAP_USERNAME:-jane@gmail.com}"
export IMAP_PASSWORD="${IMAP_PASSWORD:-app-password-here}"

# LLM API key (for classify-reply)
export ANTHROPIC_API_KEY="${ANTHROPIC_API_KEY:-sk-ant-...}"

# Consent token for automation (issue via: openeraseme grant execute --ttl 86400)
# Refresh this token regularly to maintain security
export CONSENT_TOKEN="${CONSENT_TOKEN:-}"

# Optional: CAPTCHA solving
export CAPSOLVER_API_KEY="${CAPSOLVER_API_KEY:-}"

# Project directory (auto-detected by default)
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"
```

## Log rotation

Create `cron/logrotate.conf`:

```
/home/jane/openeraseme/cron/logs/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
}
```

Install via:

```bash
sudo cp cron/logrotate.conf /etc/logrotate.d/openeraseme
```

## Email notification on errors

### Option A: cron MAILTO

Add at the top of your crontab:

```cron
MAILTO=jane@example.com
```

cron will email any stdout/stderr output from failed commands.

### Option B: Custom notification script

Create `cron/notify.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

SUBJECT="$1"
BODY="$2"

# Send via msmtp or similar
echo "Subject: [OpenEraseMe] $SUBJECT
To: jane@example.com

$BODY" | msmtp --from=cron@example.com jane@example.com
```

Then pipe errors:

```bash
0 8 * * * cd /home/jane/openeraseme && ./cron/poll-inbox.sh 2>&1 | \
  mail -s "OpenEraseMe: poll-inbox failed" jane@example.com || true
```

## Best practices

1. **Consent tokens**: Issue a 24h token for daily cron jobs. Re-issue weekly.
   ```bash
   openeraseme grant execute --ttl 86400
   ```
2. **Log rotation**: Always set up log rotation to prevent disk fill.
3. **Dry-run first**: Run new cron jobs manually once to verify the setup.
4. **Monitor failures**: Use MAILTO or notification scripts to catch errors.
5. **Stagger tasks**: Space out polling and tick to avoid rate limits.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| cron not executing | Check `cron.env` has correct paths and `PATH` is set |
| "Command not found" | Use absolute paths or set `PATH` in env.sh |
| IMAP auth fails | Refresh IMAP password; Gmail requires app-specific password |
| Consent expired | Re-issue consent token via `openeraseme grant execute --ttl <seconds>` |
| Logs growing | Set up log rotation via `logrotate` |
