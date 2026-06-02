# Cline Integration

[Cline](https://cline.bot) is a VS Code AI assistant. It does **not** support SKILL.md natively but uses `.clinerules/` and auto-detects other formats.

## Installation

### Option 1: Project-Level Rules (Recommended)

```bash
cd /path/to/symaira-eraseme
mkdir -p .clinerules
```

Create `.clinerules/00-symaira-eraseme.md`:

```markdown
---
description: Symaira EraseMe data broker removal tool
alwaysApply: false
---

# Symaira EraseMe

## Overview

Symaira EraseMe is a CLI tool for GDPR/CCPA data broker removal with:
- 1,200+ curated broker definitions
- Campaign planning and execution
- LLM-powered inbox triage
- Deadline tracking and escalation
- Quarterly re-scan workflows

## When to Use

Activate this rule when the user wants to:
- Remove personal data from data brokers
- Exercise GDPR/CCPA erasure rights
- Plan opt-out campaigns
- Track removal request status
- Handle broker replies

## Workflow

1. **Setup**: `symeraseme init-profile`
2. **Plan**: `symeraseme plan create --campaign initial --max 5`
3. **Review**: `symeraseme plan show`
4. **Execute**: `symeraseme execute --campaign initial --batch-size 5`
5. **Triage**: `symeraseme poll-inbox && symeraseme classify-reply`
6. **Tick**: `symeraseme tick`
7. **Re-scan**: Quarterly with new campaign IDs

## Key Commands

- `symeraseme init-profile` — Create identity
- `symeraseme plan create --campaign <id>` — Plan campaign
- `symeraseme execute --campaign <id>` — Send requests (needs consent)
- `symeraseme tick` — Check deadlines
- `symeraseme poll-inbox` — Fetch replies
- `symeraseme classify-reply <id>` — Classify with LLM
- `symeraseme generate-rebuttal <id>` — Generate legal rebuttal

## Best Practices

- Always `--dry-run` first
- Use `--output json` for structured data
- Start with `--batch-size 3`
- Issue consent tokens with short TTL
- Daily triage routine

## Environment

Requires: `ANTHROPIC_API_KEY`, `SYMERASEME_DATA_DIR`
```

### Option 2: Auto-Detection

Cline automatically detects these files if they exist:
- `.cursorrules` (Cursor rules)
- `.windsurfrules` (Windsurf rules)
- `AGENTS.md` (Generic agent instructions)

See [AGENTS.md](../AGENTS.md) for a comprehensive agent instructions file.

### Option 3: Global Rules

```bash
mkdir -p ~/Documents/Cline/Rules
cp .clinerules/00-symaira-eraseme.md ~/Documents/Cline/Rules/
```

## Usage

### Enable/Disable Rules

In Cline's UI, you can toggle rules on/off per conversation.

### Auto-Detection

Cline reads `.clinerules/` automatically when opening the project.

## Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## MCP Server (Advanced)

For direct tool integration, configure an MCP server in `~/.cline/data/settings/cline_mcp_settings.json`:

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

## Documentation

- [Cline Rules](https://docs.cline.bot/customization/cline-rules)
- [Cline Blog](https://cline.bot/blog/clinerules-version-controlled-shareable-and-ai-editable-instructions)
