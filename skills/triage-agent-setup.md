# Triage Agent Self-Setup

Guide for AI agents (e.g. Hermes, Claude Code, OpenClaw) to automatically set up
and register themselves as the triage operator for OpenEraseMe.

## What this does

As an AI agent, you can:
1. Install OpenEraseMe with the correct LLM provider extra
2. Configure environment variables for your provider
3. Verify the installation works
4. Register a cronjob for daily automated triage

## Prerequisites

- Python 3.11+ installed
- pip or uv available
- One of: Anthropic API key, OpenAI API key, or local Ollama instance

## One-shot setup script

Copy-paste and execute this block. It detects your provider from env vars
and installs the correct dependencies.

```bash
#!/usr/bin/env bash
set -e

# Detect provider from env
PROVIDER=${OPENERASEME_LLM_PROVIDER:-anthropic}

# Map provider name to install extra
# (anthropic deps live under the "triage" extra)
case "$PROVIDER" in
    anthropic) EXTRA="triage" ;;
    openai)    EXTRA="openai" ;;
    ollama)    EXTRA="ollama" ;;
    *)         echo "Unknown provider: $PROVIDER"; exit 1 ;;
esac

# Install OpenEraseMe with the right LLM extra
if command -v uv &> /dev/null; then
    uv pip install "openeraseme[${EXTRA}]"
else
    pip install "openeraseme[${EXTRA}]"
fi

# Verify installation
openeraseme --version

# Verify triage commands work
openeraseme classify-reply --help
openeraseme generate-rebuttal --help

echo "OpenEraseMe triage ready with provider: ${PROVIDER}"
```

## Manual step-by-step setup

### 1. Install with your LLM provider

```bash
# Anthropic (default)
pip install "openeraseme[triage]"

# OpenAI
pip install "openeraseme[openai]"

# Ollama (no extra deps needed)
pip install "openeraseme[ollama]"

# Or all providers at once
pip install "openeraseme[triage,openai,ollama]"
```

### 2. Configure environment

Create or edit `~/.config/openeraseme/.env`:

```bash
# Choose your provider
OPENERASEME_LLM_PROVIDER=anthropic   # or: openai, ollama

# Set model (optional — provider default used if omitted)
# OPENERASEME_LLM_MODEL=claude-3-5-sonnet-latest

# Provider-specific API key
ANTHROPIC_API_KEY=sk-ant-...
# OPENAI_API_KEY=sk-...
# OLLAMA_HOST=http://localhost:11434   # only for ollama
```

### 3. Verify configuration

```bash
# Check doctor output
openeraseme doctor

# Test classify (dry-run, no DB needed)
openeraseme classify-reply --help
```

### 4. Register daily cronjob

Add to crontab for automated daily triage:

```bash
# Edit crontab
crontab -e

# Add this line for daily 9am triage
0 9 * * * export OPENERASEME_LLM_PROVIDER=anthropic; export ANTHROPIC_API_KEY=$(cat ~/.config/openeraseme/.env | grep ANTHROPIC_API_KEY | cut -d= -f2); /usr/local/bin/openeraseme poll-inbox --since 1 && /usr/local/bin/openeraseme tick
```

Or use a wrapper script:

```bash
cat > ~/.local/bin/openeraseme-triage << 'EOF'
#!/usr/bin/env bash
set -e
source ~/.config/openeraseme/.env
openeraseme poll-inbox --since 1
openeraseme tick
EOF

chmod +x ~/.local/bin/openeraseme-triage

# Add to crontab
(crontab -l 2>/dev/null; echo "0 9 * * * ~/.local/bin/openeraseme-triage") | crontab -
```

## Agent self-registration checklist

After setup, confirm each item:

- [ ] `openeraseme --version` returns a version number
- [ ] `openeraseme doctor` shows LLM provider as available
- [ ] `openeraseme classify-reply --help` shows `--provider` option
- [ ] Cronjob is registered: `crontab -l | grep openeraseme`

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `No module named 'openai'` | Missing `[openai]` extra | `pip install "openeraseme[openai]"` |
| `LLM provider not available` | API key not set | Export the provider-specific key |
| `Ollama host unreachable` | Ollama not running | Start Ollama: `ollama serve` |
| `Model not found` | Wrong model name | Check provider docs for valid model IDs |
