# Daily Tick

Guide an AI agent or user through running the tick engine for deadline
tracking, reminders, and escalation.

## Prerequisites

- Database initialized (with existing removal requests)
- Campaign executed at least once

## Overview

The tick engine is the lifecycle management component. It:

- Checks deadlines for pending removal requests
- Sends reminders for expiring requests
- Escalates overdue requests
- Recommends re-scans for completed requests

## Step 1: Dry-run the tick

Always review proposed actions before applying them:

```bash
symeraseme tick --dry-run
```

### Expected output

```
Tick: 3 action(s)
  #5 [ESCALATE] Request #5 (acxiom) overdue by 14 days — escalate(DRY RUN)
  #12 [REMINDER] Request #12 (spokeo) due in 3 days — send reminder(DRY RUN)
  #18 [COMPLETE] Request #18 (oracle) passed 90d deadline — recommend re-scan(DRY RUN)
```

### JSON output

```bash
symeraseme tick --dry-run --output json
```

```json
{
  "total_actions": 3,
  "actions": [
    {
      "request_id": 5,
      "action_type": "ESCALATE",
      "description": "Request #5 (acxiom) overdue by 14 days — escalate",
      "dry_run": true
    },
    {
      "request_id": 12,
      "action_type": "REMINDER",
      "description": "Request #12 (spokeo) due in 3 days — send reminder",
      "dry_run": true
    },
    {
      "request_id": 18,
      "action_type": "COMPLETE",
      "description": "Request #18 (oracle) passed 90d deadline — recommend re-scan",
      "dry_run": true
    }
  ]
}
```

## Step 2: Apply tick actions

Once the user approves the plan:

```bash
symeraseme tick
```

### Action types

| Action | Trigger | Effect |
|--------|---------|--------|
| `REMINDER` | Request approaching deadline (3 days) | Sends a reminder event |
| `ESCALATE` | Request overdue (>7 days) | Flags for human intervention |
| `COMPLETE` | Request passed final deadline (>90 days) | Recommends quarterly re-scan |

## Step 3: View updated request status

```bash
symeraseme requests list --status OVERDUE
symeraseme requests list --status COMPLETED
symeraseme events show 5
```

## Interpreting deadline reports

Review the output with the user after each tick:

- **Green**: No overdue requests — everything on track.
- **Yellow**: Approaching deadlines — consider sending manual follow-ups.
- **Red**: Overdue requests — may need alternative contact methods or legal escalation.

## Scheduling recommendations

The tick engine should run daily. Recommended schedule:

| Timing | Purpose |
|--------|---------|
| 08:00 | Morning tick + inbox poll |
| 12:00 | Midday check (optional) |
| 18:00 | Evening tick + end-of-day summary |

For automated scheduling, see the [cron example](../examples/plain-cron/README.md).

## Best practices

1. **Run tick daily**: Consistency is key for deadline tracking.
2. **Review before apply**: Always use `--dry-run` first and review actions
   with the user.
3. **Check after execute**: Run `tick` after executing a campaign to set
   initial deadlines.
4. **Quarterly re-scans**: When `COMPLETE` actions appear, suggest a new
   campaign (see [quarterly re-scan](re-scan-quarterly.md)).

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `Tick: no actions needed.` | Nothing due or overdue | Good — everything is on track |
| No requests found | No campaigns executed | Run a campaign first |
| Unexpired deadlines | Timezone mismatch | Check system time is correct (UTC) |
