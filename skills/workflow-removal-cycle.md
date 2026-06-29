# Removal Workflow Cycle

Master orchestration guide for the complete data broker removal lifecycle.
This template ties all sub-skills together into a repeatable cycle.

## The complete cycle

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. PLAN                                                        │
│    symeraseme plan create --campaign <id>                      │
│    symeraseme plan show                                        │
│    [review with user before executing]                         │
├─────────────────────────────────────────────────────────────────┤
│ 2. EXECUTE                                                     │
│    symeraseme plan execute --campaign <id> --batch-size 5     │
│    [consent required: --yes or grant token]                    │
├─────────────────────────────────────────────────────────────────┤
│ 3. WAIT                                                        │
│    GDPR: 30 days | CCPA: 45 days | LGPD: 15 days             │
│    Run tick daily to track deadlines                           │
├─────────────────────────────────────────────────────────────────┤
│ 4. POLL INBOX (daily)                                          │
│    symeraseme poll-inbox --username <email> --since 1          │
│    symeraseme poll-inbox --username <email> --folders INBOX,Unbekannt │
├─────────────────────────────────────────────────────────────────┤
│ 5. CLASSIFY                                                    │
│    symeraseme classify-reply <request_id>                      │
├─────────────────────────────────────────────────────────────────┤
│ 6. RESPOND                                                     │
│    CONFIRMATION → log, done                                    │
│    VERIFICATION_REQUIRED → auto-confirm or manual              │
│    REJECTED → generate-rebuttal → send rebuttal                │
│    COMPLETED → mark done                                       │
├─────────────────────────────────────────────────────────────────┤
│ 7. TICK (daily)                                                │
│    symeraseme plan tick                                        │
│    [handles deadlines, reminders, escalations]                 │
├─────────────────────────────────────────────────────────────────┤
│ 8. RE-SCAN (quarterly)                                         │
│    symeraseme plan create --campaign q2-2026-rescan           │
│    [repeat from step 2]                                        │
└─────────────────────────────────────────────────────────────────┘
```

## Decision matrix

### When to plan

- First time setting up → `plan create --campaign initial`
- New quarter → `plan create --campaign q2-2026-rescan`
- User wants to target specific brokers → `plan create --campaign <id> --max N`
- User wants jurisdiction-specific → `plan create --campaign <id> --jurisdiction GDPR`

### When to execute

- Plan is reviewed and approved by user
- Consent token is available (or use `--yes` for non-interactive)
- Start with `--batch-size 3` to avoid rate limits
- Increase batch size gradually after confirming no issues

### When to poll inbox

- Daily after sending removal requests
- Brokers typically respond in 1-5 business days
- Use `--since 1` for daily checks, `--since 7` for weekly catch-up
- Use `--folders INBOX,Unbekannt,Junk` for web.de users (see #478)

### When to classify

- After each `poll-inbox` run that returns matched messages
- Batch classifications in one session to save API costs
- Check confidence score — low confidence (<0.7) may need human review

### When to generate rebuttal

- Only when classification is `REJECTED`
- Rebuttals are jurisdiction-aware (GDPR vs CCPA)
- Review generated rebuttal before sending

### When to tick

- Daily — run `plan tick` every morning
- Use `--dry-run` first to preview what will happen
- Handles: deadline tracking, reminders, escalation to DPA complaints

### When to re-scan

- Quarterly — run `plan create` with a new campaign ID
- Catches new brokers added to the registry
- Also catches brokers that re-acquired data after initial removal

## Error handling

### Consent required

```
Error: This command requires consent. Use --yes or grant a consent token.
```

Fix:
```bash
# Option 1: Interactive consent
symeraseme plan execute --campaign initial

# Option 2: Non-interactive (automation)
symeraseme plan execute --campaign initial --yes

# Option 3: Consent token (for CI/automation)
symeraseme grant execute --ttl 3600
symeraseme plan execute --campaign initial --consent <token>
```

### No identity profile

```
Error: No identity profile found. Run 'symeraseme init-profile' first.
```

Fix:
```bash
symeraseme init-profile
```

### IMAP errors

```
IMAP error: [AUTHENTICATIONFAILED] Invalid credentials
```

Fix:
- Use app-specific password (not regular password)
- For Gmail: enable 2FA, then create app password at https://myaccount.google.com/apppasswords
- For Outlook: use OAuth2 via `symeraseme accounts add outlook`
- For web.de: use `--folders INBOX,Unbekannt` (see #478)

### LLM provider not available

```
Error: No LLM provider configured
```

Fix:
```bash
# Anthropic (default)
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI
export SYMERASEME_LLM_PROVIDER=openai
export OPENAI_API_KEY="sk-..."

# Local Ollama
export SYMERASEME_LLM_PROVIDER=ollama
export SYMERASEME_LLM_MODEL=llama3.1
```

### Web form failures

Some brokers only accept opt-outs via web forms. When automation fails:
```bash
symeraseme manual-tasks list
symeraseme manual-tasks show <task_id>
# Complete manually, then:
symeraseme manual-tasks complete <task_id>
```

### Dead endpoints

If a broker's website is unreachable:
1. Check `symeraseme brokers show <broker_id>` for alternative contact methods
2. Some brokers have email fallbacks
3. If no alternative exists, the request may need manual escalation

## Agent-specific workflow tips

### Claude Code

- Skills auto-discovered from `.claude/skills/`
- Use MCP server for file redaction: `symeraseme serve`
- Session memory persists across conversations

### Hermes

- Install skill to `~/.hermes/skills/privacy-tools/symaira-eraseme/`
- Supports progressive disclosure
- Can run background tasks via `hermes run`

### GitHub Copilot / Codex CLI

- Auto-discovered from `.agents/skills/`
- Use `/skills reload` after updates
- Works in VS Code and terminal

### Cursor

- Auto-discovered from `.cursor/skills/`
- Add `.mdc` rules for enhanced context
- Skills invoked via `/symaira-eraseme`

## Automation schedule

For fully automated removal cycles, use the built-in scheduler:

```bash
# Generate scheduler configs
symeraseme generate-scheduler --output ./schedules

# Install (launchd on macOS, cron on Linux)
symeraseme schedule install

# Check status
symeraseme schedule status
```

This sets up:
- **Daily tick**: `symeraseme plan tick` at 09:00
- **Daily inbox poll**: `symeraseme poll-inbox` at 10:00
- **Weekly report**: `symeraseme generate-report` on Mondays

## Progress tracking

```bash
# Campaign status
symeraseme plan status

# Detailed view
symeraseme plan show --campaign initial

# Calendar view
symeraseme calendar --weeks 4

# Export for records
symeraseme export --format json --output campaign.json
```
