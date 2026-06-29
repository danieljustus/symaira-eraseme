---
name: symaira-eraseme
description: >
  Orchestrate GDPR/CCPA data broker removals using the Symaira EraseMe CLI.
  Automates opt-out campaigns, inbox triage, deadline tracking, and legal
  rebuttal generation against 1,200+ data brokers.
version: 1.0.0
author: Symaira
license: MIT
platforms: [macos, linux, windows]
required_environment_variables:
  - ANTHROPIC_API_KEY
  - SYMERASEME_DATA_DIR
metadata:
  hermes:
    tags: [privacy, gdpr, ccpa, data-brokers, automation]
    category: privacy-tools
  cursor:
    paths: ["**/*.md", "**/*.yaml", "**/*.json"]
  windsurf:
    alwaysApply: false
---

# Symaira EraseMe: AI Agent Skill Bundle

**Automated data broker removal tool — close your accounts, erase your data.**

This skill bundle teaches AI agents (Claude Code, OpenClaw, Cursor, Windsurf,
Hermes, GitHub Copilot, Codex, etc.) how to orchestrate GDPR/CCPA data broker
removals using the Symaira EraseMe CLI.

## When to use Symaira EraseMe

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
│    symeraseme init-profile                              │
│    (full name, email)                                    │
├──────────────────────────────────────────────────────────┤
│ 2. PLAN CAMPAIGN                                         │
│    symeraseme plan create --campaign <id>               │
│    symeraseme plan show                                 │
│    [review plan with user]                               │
├──────────────────────────────────────────────────────────┤
│ 3. EXECUTE REMOVALS                                      │
│    symeraseme execute --campaign <id> --batch-size 5    │
│    [consent prompt required for destructive ops]         │
├──────────────────────────────────────────────────────────┤
│ 4. TRIAGE REPLIES (daily)                                │
│    symeraseme poll-inbox --username <email> ...         │
│    symeraseme classify-reply <request_id>               │
├──────────────────────────────────────────────────────────┤
│ 5. HANDLE ACTIONS (as needed)                            │
│    symeraseme auto-confirm <request_id>                 │
│    symeraseme generate-rebuttal <request_id>            │
├──────────────────────────────────────────────────────────┤
│ 6. TICK / MAINTENANCE (daily)                            │
│    symeraseme tick                                      │
├──────────────────────────────────────────────────────────┤
│ 7. QUARTERLY RE-SCAN                                     │
│    symeraseme plan create --campaign q2-2026-rescan     │
│    [repeat from step 3]                                  │
└──────────────────────────────────────────────────────────┘
```

## CLI command reference

### Setup & Identity

| Command | Description |
|---------|-------------|
| `symeraseme init-profile` | Create encrypted identity profile |
| `symeraseme show-profile` | Display current identity |
| `symeraseme accounts add <provider>` | Configure email account (gmail/outlook) |
| `symeraseme accounts list` | List configured email accounts |
| `symeraseme accounts remove <email>` | Remove an email account |
| `symeraseme db-init` | Initialize the SQLite database |

### Campaign Planning

| Command | Description |
|---------|-------------|
| `symeraseme plan create --campaign <id>` | Create a removal campaign plan |
| `symeraseme plan show` | View the current plan |
| `symeraseme requests list` | List all removal requests |
| `symeraseme events show <request_id>` | View event history for a request |

### Execution

| Command | Description |
|---------|-------------|
| `symeraseme execute --campaign <id>` | Send removal requests |
| `symeraseme grant <command>` | Issue consent token for destructive ops |
| `symeraseme render-template <name>` | Preview a template |

### Inbox & Triage

| Command | Description |
|---------|-------------|
| `symeraseme poll-inbox` | Fetch and match inbox replies |
| `symeraseme classify-reply <id>` | Classify a broker reply via LLM |
| `symeraseme generate-rebuttal <id>` | Generate a rebuttal for a rejection |
| `symeraseme auto-confirm <id>` | Auto-click confirmation links |

### Web Forms & CAPTCHAs

| Command | Description |
|---------|-------------|
| `symeraseme run-web-form <broker_id>` | Run a broker's web form opt-out |
| `symeraseme solve-captcha` | Solve a CAPTCHA via external service |
| `symeraseme manual-tasks list` | List manual fallback tasks |
| `symeraseme manual-tasks show <id>` | Show manual task details |
| `symeraseme manual-tasks complete <id>` | Mark manual task as done |

### Lifecycle

| Command | Description |
|---------|-------------|
| `symeraseme tick` | Run tick engine (deadlines, reminders) |

### Output Format

All commands support `--output json` for machine-readable output:

```bash
symeraseme plan create --campaign initial --output json
symeraseme tick --dry-run --output json
symeraseme requests list --status PENDING --output json
```

## Sub-skills

- [workflow-removal-cycle.md](workflow-removal-cycle.md) — Complete removal lifecycle orchestration
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

## Agent-specific integration notes

### Claude Code
Skills auto-discovered from `.claude/skills/` symlink in project root.
See [examples/claude-code/](../examples/claude-code/) for setup.

### OpenClaw
Skills loaded via YAML definitions in `~/.config/openclaw/skills/`.
See [examples/openclaw/](../examples/openclaw/) for setup.

### Hermes
Install to `~/.hermes/skills/privacy-tools/symaira-eraseme/`.
Supports progressive disclosure (metadata → full skill → references).

### GitHub Copilot / Codex CLI
Auto-discovered from `.agents/skills/` or `~/.agents/skills/`.
Use `/skills reload` to refresh. Verify with `/skills info symaira-eraseme`.

### Cursor
Auto-discovered from `.cursor/skills/` or `.agents/skills/`.
Skills invoked automatically based on description matching or via `/symaira-eraseme`.

### Windsurf
Auto-discovered from `.windsurf/skills/` or `.agents/skills/`.
Invoke via `@symaira-eraseme` or automatic agent detection.

### Continue, Cline, Aider
These agents do not support SKILL.md natively. See [AGENTS.md](../AGENTS.md)
for adapter files and setup instructions.
