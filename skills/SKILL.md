# OpenEraseMe: AI Agent Skill Bundle

**Automated data broker removal tool — close your accounts, erase your data.**

This skill bundle teaches AI agents (Claude Code, OpenClaw, Cursor, etc.) how to
orchestrate GDPR/CCPA data broker removals using the OpenEraseMe CLI.

## When to use OpenEraseMe

Use this skill when the user wants to:

- Remove their personal data from data broker databases
- Exercise GDPR/CCPA right to erasure / right to delete
- Plan and execute a data broker opt-out campaign
- Track the status of removal requests
- Respond to broker replies (verifications, rejections)
- Schedule recurring maintenance (tick engine, quarterly re-scans)

## Workflow overview

```
┌──────────────────────────────────────────────────────────┐
│ 1. SETUP IDENTITY                                        │
│    openeraseme init-profile                              │
│    (full name, email)                                    │
├──────────────────────────────────────────────────────────┤
│ 2. PLAN CAMPAIGN                                         │
│    openeraseme plan create --campaign <id>               │
│    openeraseme plan show                                 │
│    [review plan with user]                               │
├──────────────────────────────────────────────────────────┤
│ 3. EXECUTE REMOVALS                                      │
│    openeraseme execute --campaign <id> --batch-size 5    │
│    [consent prompt required for destructive ops]         │
├──────────────────────────────────────────────────────────┤
│ 4. TRIAGE REPLIES (daily)                                │
│    openeraseme poll-inbox --username <email> ...         │
│    openeraseme classify-reply <request_id>               │
├──────────────────────────────────────────────────────────┤
│ 5. HANDLE ACTIONS (as needed)                            │
│    openeraseme auto-confirm <request_id>                 │
│    openeraseme generate-rebuttal <request_id>            │
├──────────────────────────────────────────────────────────┤
│ 6. TICK / MAINTENANCE (daily)                            │
│    openeraseme tick                                      │
├──────────────────────────────────────────────────────────┤
│ 7. QUARTERLY RE-SCAN                                     │
│    openeraseme plan create --campaign q2-2026-rescan     │
│    [repeat from step 3]                                  │
└──────────────────────────────────────────────────────────┘
```

## CLI command reference

### Setup & Identity

| Command | Description |
|---------|-------------|
| `openeraseme init-profile` | Create encrypted identity profile |
| `openeraseme show-profile` | Display current identity |
| `openeraseme accounts add <provider>` | Configure email account (gmail/outlook) |
| `openeraseme accounts list` | List configured email accounts |
| `openeraseme accounts remove <email>` | Remove an email account |
| `openeraseme db-init` | Initialize the SQLite database |

### Campaign Planning

| Command | Description |
|---------|-------------|
| `openeraseme plan create --campaign <id>` | Create a removal campaign plan |
| `openeraseme plan show` | View the current plan |
| `openeraseme requests list` | List all removal requests |
| `openeraseme events show <request_id>` | View event history for a request |

### Execution

| Command | Description |
|---------|-------------|
| `openeraseme execute --campaign <id>` | Send removal requests |
| `openeraseme grant <command>` | Issue consent token for destructive ops |
| `openeraseme render-template <name>` | Preview a template |

### Inbox & Triage

| Command | Description |
|---------|-------------|
| `openeraseme poll-inbox` | Fetch and match inbox replies |
| `openeraseme classify-reply <id>` | Classify a broker reply via LLM |
| `openeraseme generate-rebuttal <id>` | Generate a rebuttal for a rejection |
| `openeraseme auto-confirm <id>` | Auto-click confirmation links |

### Web Forms & CAPTCHAs

| Command | Description |
|---------|-------------|
| `openeraseme run-web-form <broker_id>` | Run a broker's web form opt-out |
| `openeraseme solve-captcha` | Solve a CAPTCHA via external service |
| `openeraseme manual-tasks list` | List manual fallback tasks |
| `openeraseme manual-tasks show <id>` | Show manual task details |
| `openeraseme manual-tasks complete <id>` | Mark manual task as done |

### Lifecycle

| Command | Description |
|---------|-------------|
| `openeraseme tick` | Run tick engine (deadlines, reminders) |

### Output Format

All commands support `--output json` for machine-readable output:

```bash
openeraseme plan create --campaign initial --output json
openeraseme tick --dry-run --output json
openeraseme requests list --status PENDING --output json
```

## Sub-skills

- [setup-identity.md](setup-identity.md) — Creating and managing your identity vault
- [plan-removal-campaign.md](plan-removal-campaign.md) — Planning a removal campaign
- [send-removal-batch.md](send-removal-batch.md) — Sending removal requests
- [triage-broker-replies.md](triage-broker-replies.md) — Daily inbox triage workflow
- [handle-action-required.md](handle-action-required.md) — Handling verifications and rejections
- [daily-tick.md](daily-tick.md) — Running the tick engine
- [re-scan-quarterly.md](re-scan-quarterly.md) — Quarterly re-scan workflow

## Error handling

- **Consent required**: Destructive commands (`execute`) require `--yes` or a consent token.
- **No profile**: Run `init-profile` first if commands fail with "No identity profile found."
- **No database**: Commands auto-init the database, but `db-init` can be run manually.
- **API key missing**: `classify-reply` and `generate-rebuttal` need `ANTHROPIC_API_KEY`.
- **IMAP errors**: Check credentials and app-specific password for Gmail/Outlook.
- **Web form failures**: Use `manual-tasks list` to find fallback tasks, then complete them.

## Best practices

1. **Always dry-run first**: Use `--dry-run` with `execute` and `tick` before real execution.
2. **Batch sizes**: Start with `--batch-size 3` to avoid rate limits; increase gradually.
3. **Consent tokens**: Issue short-lived tokens with `grant execute --ttl 3600` for automation.
4. **Daily triage**: Run `poll-inbox` + `classify-reply` daily to catch broker responses.
5. **Review plans**: Always `plan show` and review with the user before executing.
6. **Quarterly re-scans**: Run `plan create` with a new campaign ID every quarter to catch new brokers.
