# Codex CLI Integration

[OpenAI Codex CLI](https://github.com/openai/codex) supports SKILL.md with the agentskills.io standard.

## Installation

### Option 1: Project-Level (Recommended)

The repository includes a pre-configured `.agents/skills/` directory:

```bash
cd /path/to/symaira-eraseme
# Already configured: .agents/skills/symaira-eraseme -> ../../skills/
```

### Option 2: User-Level

```bash
# Create user skills directory
mkdir -p ~/.codex/skills

# Clone or symlink
git clone https://github.com/danieljustus/Symaira-EraseMe.git
cd symaira-eraseme
ln -sf $(pwd)/skills ~/.codex/skills/symaira-eraseme
```

### Option 3: Global Agents Directory

```bash
mkdir -p ~/.agents/skills
ln -sf /path/to/symaira-eraseme/skills ~/.agents/skills/symaira-eraseme
```

## Metadata File (Optional)

For enhanced UI integration, create `agents/openai.yaml` in your project:

```yaml
interface:
  display_name: "EraseMe"
  short_description: "GDPR/CCPA data broker removal tool"
  icon_small: "./assets/small-logo.svg"
  icon_large: "./assets/large-logo.png"
  brand_color: "#3B82F6"
policy:
  allow_implicit_invocation: true
dependencies:
  tools:
    - type: "mcp"
      value: "symeraseme"
```

## Usage

### Reload skills

```bash
codex /skills reload
```

### Verify skill is loaded

```bash
codex /skills info symaira-eraseme
```

### Use the skill

```bash
codex "Help me remove my data from data brokers using Symaira EraseMe"
```

## Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skill not found | Run `codex /skills reload` |
| Command not found | `pip install symeraseme` or `uv pip install -e .` |
| JSON parsing errors | Use `--output json` flag with symeraseme commands |

## Documentation

- [Codex Customization](https://developers.openai.com/codex/concepts/customization)
- [Codex Skills](https://developers.openai.com/codex/skills)
