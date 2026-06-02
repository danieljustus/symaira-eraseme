# Windsurf Integration

[Windsurf](https://codeium.com/windsurf) by Codeium supports **Skills** (`.windsurf/skills/*/SKILL.md`), **Rules** (`.windsurf/rules/*.md`), and **Workflows** (`.windsurf/workflows/*.md`).

## Installation

### Option 1: Project-Level Skills (Recommended)

```bash
cd /path/to/symaira-eraseme
mkdir -p .windsurf/skills
ln -sf ../../skills .windsurf/skills/symaira-eraseme
```

### Option 2: User-Level Skills

```bash
mkdir -p ~/.codeium/windsurf/skills
ln -sf /path/to/symaira-eraseme/skills ~/.codeium/windsurf/skills/symaira-eraseme
```

### Option 3: Auto-Discovery

Windsurf also discovers skills from `.agents/skills/` (already configured in this repo).

## Rules (Optional Enhancement)

Create `.windsurf/rules/symaira-eraseme.md`:

```markdown
---
description: Symaira EraseMe data broker removal guidelines
---

# Symaira EraseMe Rules

When working with data broker removal:
1. Always use `symeraseme --output json` for structured data
2. Dry-run before destructive operations
3. Respect consent requirements
4. Use batch sizes of 3-5 for rate limiting
```

## Workflows (Optional Enhancement)

Create `.windsurf/workflows/remove-data.md`:

```markdown
# Data Broker Removal Workflow

1. Run `symeraseme init-profile` to set up identity
2. Run `symeraseme plan create --campaign initial --max 5`
3. Review plan with user
4. Execute with `symeraseme execute --campaign initial --batch-size 5`
5. Set up daily triage: `symeraseme poll-inbox && symeraseme tick`
```

## Usage

### Invoke via Cascade

Type `@symaira-eraseme` in the Cascade chat or simply describe your request:
- "Help me remove my data from data brokers"
- "Start a GDPR removal campaign"

### Auto-invocation

Windsurf's agent will automatically detect when to use the skill based on the description in SKILL.md.

## Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skill not found | Check `.windsurf/skills/symaira-eraseme/SKILL.md` exists |
| Cascade not invoking | Restart Windsurf or reload window |
| Commands not found | Ensure `symeraseme` is installed globally |

## Documentation

- [Windsurf Skills](https://docs.windsurf.com/windsurf/cascade/skills)
- [Windsurf Rules](https://windsurf.com/university/general-education/intro-rules-memories)
