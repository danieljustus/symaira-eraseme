# Triage Agent Self-Setup

Guide for AI agents (e.g. Hermes, Claude Code, OpenClaw) to automatically set up
and register themselves as the triage operator for Symaira EraseMe.

## What this does

As an AI agent, you can:
1. Install Symaira EraseMe with the correct LLM provider extra
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
PROVIDER=${SYMERASEME_LLM_PROVIDER:-anthropic}

# Map provider name to install extra
# (anthropic deps live under the "triage" extra)
case "$PROVIDER" in
    anthropic) EXTRA="triage" ;;
    openai)    EXTRA="openai" ;;
    ollama)    EXTRA="ollama" ;;
    *)         echo "Unknown provider: $PROVIDER"; exit 1 ;;
esac

# Install Symaira EraseMe with the right LLM extra
if command -v uv &> /dev/null; then
    uv pip install "symeraseme[${EXTRA}]"
else
    pip install "symeraseme[${EXTRA}]"
fi

# Verify installation
symeraseme --version

# Verify triage commands work
symeraseme classify-reply --help
symeraseme generate-rebuttal --help

echo "Symaira EraseMe triage ready with provider: ${PROVIDER}"
```

## Manual step-by-step setup

### 1. Install with your LLM provider

```bash
# Anthropic (default)
pip install "symeraseme[triage]"

# OpenAI
pip install "symeraseme[openai]"

# Ollama (no extra deps needed)
pip install "symeraseme[ollama]"

# Or all providers at once
pip install "symeraseme[triage,openai,ollama]"
```

### 2. Configure environment

Create or edit `~/.config/symeraseme/.env`:

```bash
# Choose your provider
SYMERASEME_LLM_PROVIDER=anthropic   # or: openai, ollama

# Set model (optional — provider default used if omitted)
# SYMERASEME_LLM_MODEL=claude-3-5-sonnet-latest

# Provider-specific API key
ANTHROPIC_API_KEY=sk-ant-...
# OPENAI_API_KEY=sk-...
# OLLAMA_HOST=http://localhost:11434   # only for ollama
```

### 3. Verify configuration

```bash
# Check doctor output
symeraseme doctor

# Test classify (dry-run, no DB needed)
symeraseme classify-reply --help
```

### 4. Register daily cronjob

Add to crontab for automated daily triage:

```bash
# Edit crontab
crontab -e

# Add this line for daily 9am triage
0 9 * * * export SYMERASEME_LLM_PROVIDER=anthropic; export ANTHROPIC_API_KEY=$(cat ~/.config/symeraseme/.env | grep ANTHROPIC_API_KEY | cut -d= -f2); /usr/local/bin/symeraseme poll-inbox --since 1 && /usr/local/bin/symeraseme tick
```

Or use a wrapper script:

```bash
cat > ~/.local/bin/symeraseme-triage << 'EOF'
#!/usr/bin/env bash
set -e
source ~/.config/symeraseme/.env
symeraseme poll-inbox --since 1
symeraseme tick
EOF

chmod +x ~/.local/bin/symeraseme-triage

# Add to crontab
(crontab -l 2>/dev/null; echo "0 9 * * * ~/.local/bin/symeraseme-triage") | crontab -
```

## Agent self-registration checklist

After setup, confirm each item:

- [ ] `symeraseme --version` returns a version number
- [ ] `symeraseme doctor` shows LLM provider as available
- [ ] `symeraseme classify-reply --help` shows `--provider` option
- [ ] Cronjob is registered: `crontab -l | grep symeraseme`

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `No module named 'openai'` | Missing `[openai]` extra | `pip install "symeraseme[openai]"` |
| `LLM provider not available` | API key not set | Export the provider-specific key |
| `Ollama host unreachable` | Ollama not running | Start Ollama: `ollama serve` |
| `Model not found` | Wrong model name | Check provider docs for valid model IDs |
