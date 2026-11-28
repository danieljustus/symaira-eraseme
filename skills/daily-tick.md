# Daily Tick

Guide an AI agent or user through running the tick engine for deadline
tracking, reminders, and escalation.

## Prerequisites

- Database initialized (with existing removal requests)
- Campaign executed at least once

## Overview

The tick engine is the lifecycle management component. Each run evaluates the
requests whose next action is due:

| Action | Trigger | What a non-dry `tick` does |
|--------|---------|----------------------------|
| `mark_overdue` | `AWAITING_RESPONSE` and the response deadline has passed | Records `DEADLINE_REACHED`; the request becomes `OVERDUE` |
| `trigger_rescan` | `CONFIRMED` 90 or more days ago | Records `RE_SCAN_TRIGGERED`; the request becomes `RE_SCAN_DUE` |
| `send_reminder` | `AWAITING_ACK` 7, 14, 28, ... days after sending | Queues a `reminder_due` manual task |
| `draft_dpa_complaint` | `OVERDUE` for 14 or more days after the deadline | Queues a `dpa_escalation_due` manual task |

`tick` never sends a reminder or drafts a complaint itself, and it never records
one that did not happen. A queued task moves the request to
`AWAITING_USER_ACTION`, so later ticks do not queue it again. Completing the task
records `REMINDER_SENT` or `DPA_COMPLAINT_DRAFTED`.

## Step 1: Dry-run the tick

Review the due actions before applying them:

```bash
symeraseme tick --dry-run
symeraseme tick --dry-run --output json
```

Text mode prints `tick complete: N action(s)`. JSON lists each action:

```json
{"actions":[{"RequestID":12,"BrokerID":"spokeo","CampaignID":"initial","CurrentStatus":"AWAITING_ACK","ActionType":"send_reminder","EventType":"REMINDER_SENT","Description":"Send reminder #1 (9d since sent)","Payload":{"count":1,"days_since_sent":9},"DryRun":true}],"dry_run":true,"success":true}
```

`EventType` names the event the action stands for. For `send_reminder` and
`draft_dpa_complaint` it is recorded only when the manual task is completed.

## Step 2: Apply tick actions

Once the user approves the plan:

```bash
symeraseme tick
```

Text mode adds one line per action, for example:

```
tick complete: 2 action(s)
  request #5: recorded DEADLINE_REACHED
  request #12: send_reminder queued as manual task #3
```

With `--output json` the same outcomes appear under `applied`
(`request_id`, `action`, `event_type`, `manual_task_id`, `error`). Without
due actions the output is unchanged: `tick complete: 0 action(s)`.

## Step 3: Work the queued tasks

```bash
symeraseme manual-tasks list --status pending
symeraseme manual-tasks show 3
```

- `reminder_due`: send the reminder to the broker through the channel the
  request used.
- `dpa_escalation_due`: prepare the complaint to the data protection authority.

Then mark the task done; this records the event:

```bash
symeraseme manual-tasks complete 3 --notes "reminder sent by email"
```

If the broker replied while the task was pending, completing it only adds a
note, so the newer status stays.

## Step 4: View updated request status

```bash
symeraseme requests list --status OVERDUE
symeraseme requests list --status AWAITING_USER_ACTION
symeraseme events show 5
```

## Scheduling

The generated scheduler jobs run `symeraseme tick --output json` once a day,
which applies the actions as above. Check `manual-tasks list --status pending`
after the scheduled run. For automated scheduling, see `schedule install` or
the [cron example](../examples/plain-cron/README.md).

## Best practices

1. **Run tick daily**: Consistency is key for deadline tracking.
2. **Review before apply**: Use `--dry-run` first when running it by hand.
3. **Close the loop**: Complete reminder and complaint tasks once they are
   done, so the next reminder threshold and the escalation level advance.
4. **Quarterly re-scans**: When requests reach `RE_SCAN_DUE`, suggest a new
   campaign (see [quarterly re-scan](re-scan-quarterly.md)).

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `tick complete: 0 action(s)` | Nothing due | Good: everything is on track |
| A line ends in `failed: <error>` | That action could not be written | Fix the cause and run `tick` again; the other actions were applied |
| No requests found | No campaigns executed | Run a campaign first |
