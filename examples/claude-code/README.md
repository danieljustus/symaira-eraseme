# Claude Code Integration

This example shows how to configure Claude Code to use Symaira EraseMe's
skill bundle for automated data broker removal orchestration.

## Prerequisites

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed
- Symaira EraseMe installed (`uv sync`)
- Python 3.11+

## Setup

### 1. Link the skill bundle

Claude Code discovers skills from `.claude/skills/` in the project root.
A pre-configured symlink is included:

```bash
ls -la .claude/skills/
# symeraseme -> ../../skills/
```

If you're setting up manually:

```bash
mkdir -p .claude/skills
ln -sf ../../skills .claude/skills/symeraseme
```

### 2. Verify skills are loaded

Start Claude Code in the project directory:

```bash
cd /path/to/symeraseme
claude
```

Ask Claude:
> "What skills are available for Symaira EraseMe?"

Claude should list the skill bundle and describe how to orchestrate
data broker removals.

### 3. Run the setup workflow

Tell Claude:

> "I want to start removing my data from data brokers. Help me set up
> my identity profile and plan a campaign."

Claude will use the skills to walk through:
1. `symeraseme init-profile` — Create your identity vault
2. `symeraseme plan create` — Scan the broker registry
3. `symeraseme plan show` — Review the plan with you
4. `symeraseme execute` — Send removal requests

## Example session

```
User: Help me remove my data from data brokers using Symaira EraseMe.

Claude: I'll help you with that! Let me start by checking your setup.

Let me check if you have an identity profile configured...

→ symeraseme show-profile

[No identity profile found]

First, let's create your identity profile. I'll need your full name
and email address.

→ symeraseme init-profile
Full name: Jane Doe
Email address: jane@example.com

[Created encrypted identity profile at ~/.symeraseme/identity.json]

Great! Now let's plan a campaign...

→ symeraseme plan create --campaign initial --max 5

[Campaign: initial]
[Total brokers scanned: 32]
[Planned requests: 5]

Here's the plan:
  #1 Acxiom (email) — GDPR jurisdiction
  #2 Spokeo (web_form) — CCPA jurisdiction
  #3 Oracle (email) — GDPR jurisdiction
  #4 Acxiom (web_form) — CCPA jurisdiction
  #5 Verisk (email) — GDPR jurisdiction

Shall I proceed with executing these 5 requests?
...

User: Yes, please execute them.

Claude: I need your consent for this destructive operation. Please confirm:

→ symeraseme execute --campaign initial --batch-size 5
This is a destructive operation. Continue? [y/N]: y

[Results:]
  #1 OK — sent via email
  #2 OK — web form submitted
  #3 OK — sent via email
  #4 OK — web form submitted
  #5 OK — sent via email
```

## Configuration recommendations

For best results with Claude Code:

### Project-level config

Add to `.claude/settings.json`:

```json
{
  "skills": {
    "symeraseme": {
      "enabled": true,
      "path": ".claude/skills/symeraseme"
    }
  }
}
```

### Environment variables

Set these in your shell profile or `.env`:

```bash
# Required for LLM-powered features
export ANTHROPIC_API_KEY="sk-ant-..."
# Required for CAPTCHA solving
export CAPSOLVER_API_KEY="CAP-..."
# Optional: override data directory
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

### Claude Code MCP configuration

For direct tool access (advanced), add to `.claude/mcp.json`:

```json
{
  "mcpServers": {
    "symeraseme": {
      "command": "uv",
      "args": ["run", "symeraseme"],
      "env": {
        "ANTHROPIC_API_KEY": "${ANTHROPIC_API_KEY}",
        "SYMERASEME_DATA_DIR": "${HOME}/.symeraseme"
      }
    }
  }
}
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skills not loading | Ensure `.claude/skills/symeraseme` is a valid symlink to `skills/` |
| Command not found | Run `uv sync && uv pip install -e .` |
| API key errors | Check `ANTHROPIC_API_KEY` is set in the environment |
| IMAP connection fails | Use an app-specific password for Gmail/Outlook |
