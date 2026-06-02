# Aider Integration

[Aider](https://aider.chat) is a terminal-based AI pair programming tool. It does **not** have a formal skill system but supports convention files via `--read`.

## Installation

### Option 1: Project-Level Conventions

Create `CONVENTIONS.md` in your project root:

```markdown
# Symaira EraseMe Conventions

## Overview

Symaira EraseMe is a CLI tool for GDPR/CCPA data broker removal.

## Commands

### Setup
```bash
symeraseme init-profile        # Create encrypted identity
symeraseme show-profile        # Display identity
```

### Campaign Planning
```bash
symeraseme plan create --campaign initial --max 5
symeraseme plan show
symeraseme requests list
```

### Execution
```bash
symeraseme execute --campaign initial --batch-size 5 --dry-run  # Always dry-run first
symeraseme grant execute --ttl 3600                             # Issue consent token
symeraseme execute --campaign initial --batch-size 5 --consent <token>
```

### Triage
```bash
symeraseme poll-inbox --username user@gmail.com --since 3
symeraseme classify-reply <request_id>
symeraseme generate-rebuttal <request_id>
```

### Lifecycle
```bash
symeraseme tick --dry-run
symeraseme tick
```

## Best Practices

1. Always dry-run before destructive operations
2. Use `--output json` for machine-readable output
3. Start with small batch sizes (3-5)
4. Issue short-lived consent tokens
5. Run daily triage
6. Quarterly re-scans

## Environment

Set these variables:
- `ANTHROPIC_API_KEY` — For LLM triage
- `SYMERASEME_DATA_DIR` — Data directory
```

### Option 2: Auto-Load via Config

Create `.aider.conf.yml`:

```yaml
read:
  - CONVENTIONS.md

# Optional: Set model
model: claude-3-5-sonnet-20241022
```

### Option 3: Global Conventions

```bash
mkdir -p ~/.config/aider
cp CONVENTIONS.md ~/.config/aider/GLOBAL_CONVENTIONS.md
```

Add to `~/.aider.conf.yml`:

```yaml
read:
  - ~/.config/aider/GLOBAL_CONVENTIONS.md
```

## Usage

### With --read flag

```bash
aider --read CONVENTIONS.md
```

### With config file

```bash
aider  # Automatically loads .aider.conf.yml
```

### Instruct Aider

Once loaded, Aider will follow the conventions when generating code or commands.

## Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## AI Comments (Optional)

Add special comments to trigger Aider actions:

```python
# AI! Implement the plan execution function
def execute_plan(campaign_id):
    pass
```

## Documentation

- [Aider Conventions](https://aider.chat/docs/usage/conventions.html)
- [Aider Config](https://aider.chat/docs/config/aider_conf.html)
