# Cursor Integration

[Cursor](https://cursor.com) supports both **Skills** (`.cursor/skills/*/SKILL.md`) and **Rules** (`.cursor/rules/*.mdc`).

## Installation

### Option 1: Project-Level Skills (Recommended)

```bash
cd /path/to/symaira-eraseme
mkdir -p .cursor/skills
ln -sf ../../skills .cursor/skills/symaira-eraseme
```

### Option 2: User-Level Skills

```bash
mkdir -p ~/.cursor/skills
ln -sf /path/to/symaira-eraseme/skills ~/.cursor/skills/symaira-eraseme
```

### Option 3: Auto-Discovery via .agents/

Cursor also discovers skills from `.agents/skills/` (already configured in this repo).

## Rules (Optional Enhancement)

For additional context, create `.cursor/rules/symaira-eraseme.mdc`:

```markdown
---
description: Symaira EraseMe data broker removal rules
globs: ["**/*.md", "**/*.yaml"]
alwaysApply: false
---

# Symaira EraseMe Rules

When working with data broker removal:
1. Always use `symeraseme --output json` for structured data
2. Dry-run before destructive operations
3. Respect consent requirements
4. Use batch sizes of 3-5 for rate limiting
```

## Usage

### Invoke via command palette

1. Open Cursor Command Palette (`Cmd+Shift+P` or `Ctrl+Shift+P`)
2. Type `/symaira-eraseme`
3. Or type your request: "Help me remove my data from data brokers"

### Auto-invocation

Cursor will automatically load the skill when your message matches the description:
- "remove my data from brokers"
- "GDPR data removal"
- "opt out of data brokers"

## Import from GitHub

1. Open Cursor Settings → Rules
2. Click "Add Rule" → "Remote Rule"
3. Paste: `https://github.com/danieljustus/Symaira-EraseMe`
4. Select the skills directory

## Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skill not appearing | Check `.cursor/skills/symaira-eraseme/SKILL.md` exists |
| Rules not loading | Verify `.mdc` file has valid frontmatter |
| Agent not invoking | Ensure description contains relevant keywords |

## Documentation

- [Cursor Skills](https://cursor.com/docs/skills)
- [Cursor Rules](https://cursor.com/docs/rules)
