---
name: symaira-eraseme
description: >
  Orchestrate GDPR/CCPA data broker removals using the Symaira EraseMe CLI.
  Automates opt-out campaigns, inbox triage, deadline tracking, and legal
  rebuttal generation against 1,200+ data brokers.
version: 1.0.0
author: Symaira
license: Apache-2.0
platforms: [macos, linux, windows]
required_environment_variables: []
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
│ 3. RUN THE PLAN                                          │
│    symeraseme plan execute --campaign <id> --dry-run     │
│    [Rust CLI has no outbound email/browser executor]     │
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

Inbox polling uses `IMAP_*` settings or the documented `poll-inbox` overrides.
The default Rust CLI does not inject an SMTP sender into `plan execute`, so
outgoing SMTP environment variables do not enable email sending; email-based
execution requires a separately injected sender. There is no account-management
command group or separate database-init command; store-backed commands open and
initialize the configured store.

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
| `symeraseme plan execute --campaign <id> [--dry-run]` | Dry-run previews; the default CLI has no outbound adapters—email requests need an injected sender, while web forms become manual tasks |
| `symeraseme grant <command>` | Issue consent token for non-dry execution |
| `symeraseme render-template <name>` | Preview a template |

### Inbox & Triage

| Command | Description |
|---------|-------------|
| `symeraseme poll-inbox` | Fetch and match inbox replies |
| `symeraseme classify-reply <id>` | Classify a broker reply via LLM |
| `symeraseme generate-rebuttal <id>` | Generate a rebuttal for a rejection |
| `symeraseme auto-confirm <request_id>` | Create a manual confirmation task unless a browser executor is explicitly injected |

### Web Forms & CAPTCHAs

| Command | Description |
|---------|-------------|
| `symeraseme run-web-form <broker_id>` | Preview or queue a broker's manual form opt-out |
| `symeraseme manual-tasks list` | List manual fallback tasks |
| `symeraseme manual-tasks show <id>` | Show manual task details |
| `symeraseme manual-tasks complete <id>` | Mark manual task as done |

Browser execution and CAPTCHA solving are not implemented by these CLI
commands. Complete the broker form manually and record the result through the
manual-task workflow.

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

- **Consent required**: `plan execute` requires an explicit `execute` consent token unless `--dry-run` is set. Issue one with `grant execute`, then supply `--consent` or `--consent-file`; there is no `--yes` bypass or implicit prompt. The default Rust CLI still has no outbound email/browser adapters.
- **No profile**: Run `init-profile` first if commands fail with "No identity profile found."
- **No database**: Store-backed commands initialize the database on first use.
  The Rust CLI defaults to `~/.local/share/symeraseme`; `SYMERASEME_DATA_DIR`
  is an optional base-directory override. Check directory permissions if open
  fails.
- **LLM configuration:** Needed only for LLM-backed features. The Rust provider
  defaults to Anthropic (`ANTHROPIC_API_KEY`); select another backend with
  `SYMERASEME_LLM_PROVIDER`. OpenAI uses `OPENAI_API_KEY`, Ollama can use
  `OLLAMA_HOST`, and `openai-compatible` requires `SYMERASEME_LLM_BASE_URL`.
  The `agent` provider uses a configured local agent backend. These credentials
  are not prerequisites for other CLI commands.
- **IMAP errors**: Check credentials and app-specific password for Gmail/Outlook.
- **Web form failures**: Use `manual-tasks list` to find fallback tasks, then complete them.

## Best practices

1. **Always dry-run first**: Preview with `--dry-run`. The default Rust CLI does not send email or submit forms; web-form requests become manual tasks.
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
