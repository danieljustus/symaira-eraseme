# AI Agent Integration Guide

## Current product contract

[PB-2026-09-09](docs/product-boundaries.md) keeps EraseMe an independent specialized product. Browse and Operate become optional Brain modules; the credential service remains independently usable. None becomes a mandatory Brain gateway/context dependency for EraseMe. Existing command/API, privacy, evidence and approval contracts remain unchanged until explicit tested migrations. Do not couple privacy workflows to the presence of Brain's GUI.

**Symaira EraseMe** supports all major AI coding agents through standardized
skill formats and adapter files.

The unreleased 0.14.0 candidate uses the Rust CLI and a native SwiftUI macOS app,
without a bundled Go backend or backend selector. Rust 1.98.0 is required for development;
the Go implementation remains a temporary development oracle until the separate
CUT-005 retirement after stable-release observation. Historical `v0.13.0` assets
stay immutable; Rust-only candidates require fresh native and release evidence.

## Ecosystem Guidance

- Before changing cross-tool integrations, shared conventions, or product
  boundaries, read [PB-2026-09-09](docs/product-boundaries.md); it is available in standalone checkouts.
- Keep the standalone-first contract: this repo must install, test, and run
  without any other Symaira tool installed.

## Quick Reference

| Agent | Format | Auto-Discovery | Setup Complexity |
|-------|--------|----------------|------------------|
| [Claude Code](#claude-code) | `SKILL.md` | ✅ `.claude/skills/` | Easy |
| [OpenClaw](#openclaw) | YAML | Manual load | Medium |
| [Hermes](#hermes) | `SKILL.md` | `~/.hermes/skills/` | Easy |
| [GitHub Copilot CLI](#github-copilot-cli) | `SKILL.md` | `.agents/skills/` | Easy |
| [Codex CLI](#codex-cli) | `SKILL.md` | `.agents/skills/` | Easy |
| [Cursor](#cursor) | `SKILL.md` + `.mdc` | `.cursor/skills/` | Easy |
| [Windsurf](#windsurf) | `SKILL.md` + `.md` | `.windsurf/skills/` | Easy |
| [Continue](#continue) | `.md` rules | `.continue/rules/` | Medium |
| [Cline](#cline) | `.md` rules | `.clinerules/` | Medium |
| [Aider](#aider) | `CONVENTIONS.md` | Manual `--read` | Medium |

## Cross-Agent Compatibility

Five agents support the **SKILL.md** standard natively:
- Hermes, GitHub Copilot CLI, Codex CLI, Cursor, Windsurf

These agents auto-discover from `.agents/skills/`. The symlink is not tracked:
run `./scripts/setup-agents.sh --agent all` to create it.

## Skill Bundle Contents

The skill bundle (`skills/SKILL.md` + sub-skills) includes:

- **SKILL.md** — Main skill definition with CLI command reference
- **workflow-removal-cycle.md** — Complete removal lifecycle orchestration guide
- **setup-identity.md** — Identity vault setup
- **plan-removal-campaign.md** — Campaign planning
- **send-removal-batch.md** — Sending removal requests
- **triage-broker-replies.md** — Daily inbox triage workflow
- **handle-action-required.md** — Handling verifications and rejections
- **daily-tick.md** — Running the tick engine
- **re-scan-quarterly.md** — Quarterly re-scan workflow

The **workflow-removal-cycle.md** template ties all sub-skills together into a repeatable cycle: plan → execute → wait → poll → classify → respond → tick → re-scan. It includes a decision matrix for when to use each command and error handling guidance.

## Agent-Specific Setup

### Claude Code

**Format**: `SKILL.md`  
**Path**: `.claude/skills/symaira-eraseme/`  
**Status**: Run the setup script to create the untracked symlink

```bash
cd /path/to/symaira-eraseme
./scripts/setup-agents.sh --agent claude
ls -la .claude/skills/
# symaira-eraseme -> ../../skills
```

See [examples/claude-code/](examples/claude-code/) for details.

### OpenClaw

**Format**: YAML  
**Path**: `~/.config/openclaw/skills/symeraseme.yaml`  
**Status**: Manual install required

```bash
# Copy YAML skill definition
cp examples/openclaw/symeraseme.yaml ~/.config/openclaw/skills/
openclaw skill load symeraseme
```

See [examples/openclaw/](examples/openclaw/) for details.

### Hermes

**Format**: `SKILL.md`  
**Path**: `~/.hermes/skills/privacy-tools/symaira-eraseme/`  
**Status**: Manual install required

```bash
hermes skills install https://raw.githubusercontent.com/danieljustus/Symaira-EraseMe/main/skills/SKILL.md
```

Or manually:
```bash
mkdir -p ~/.hermes/skills/privacy-tools/symaira-eraseme
cp skills/SKILL.md ~/.hermes/skills/privacy-tools/symaira-eraseme/
```

See [examples/hermes/](examples/hermes/) for details.

### GitHub Copilot CLI

**Format**: `SKILL.md`  
**Path**: `.agents/skills/` or `~/.copilot/skills/`  
**Status**: Auto-discovered after creating the untracked `.agents/skills/` symlink

```bash
./scripts/setup-agents.sh --agent codex
ls -la .agents/skills/
# symaira-eraseme -> ../../skills
```

Verify:
```bash
copilot /skills reload
copilot /skills info symaira-eraseme
```

### Codex CLI

**Format**: `SKILL.md`  
**Path**: `.agents/skills/` or `~/.codex/skills/`  
**Status**: Auto-discovered after creating the untracked `.agents/skills/` symlink

```bash
./scripts/setup-agents.sh --agent codex
codex /skills reload
codex /skills info symaira-eraseme
```

See [examples/codex/](examples/codex/) for optional metadata file.

### Cursor

**Format**: `SKILL.md` (skills) + `.mdc` (rules)  
**Path**: `.cursor/skills/` or `.agents/skills/`  
**Status**: Auto-discovered after running the setup script

Create the skills symlink:
```bash
./scripts/setup-agents.sh --agent cursor
```

This checkout does not include an optional `.mdc` rules template.

See [examples/cursor/](examples/cursor/) for details.

### Windsurf

**Format**: `SKILL.md` (skills) + `.md` (rules)  
**Path**: `.windsurf/skills/` or `.agents/skills/`  
**Status**: Auto-discovered after running the setup script

Create the skills symlink:
```bash
./scripts/setup-agents.sh --agent windsurf
```

This checkout does not include optional Windsurf rules/workflow templates.

See [examples/windsurf/](examples/windsurf/) for details.

### Continue

**Format**: `.md` rules  
**Path**: `.continue/rules/`  
**Status**: Manual setup required

```bash
mkdir -p .continue/rules
cp examples/continue/.continue/rules/symaira-eraseme.md .continue/rules/
```

Optional: Create `.continuerc.json` for project config.

See [examples/continue/](examples/continue/) for details.

### Cline

**Format**: `.md` rules  
**Path**: `.clinerules/`  
**Status**: Manual setup required

```bash
mkdir -p .clinerules
cp examples/cline/.clinerules/00-symaira-eraseme.md .clinerules/
```

Cline also auto-detects `AGENTS.md`, `.cursorrules`, and `.windsurfrules`.

See [examples/cline/](examples/cline/) for details.

### Aider

**Format**: `CONVENTIONS.md`  
**Path**: Project root or `~/.config/aider/`  
**Status**: Manual setup required

```bash
cp examples/aider/CONVENTIONS.md ./CONVENTIONS.md
```

Add to `.aider.conf.yml`:
```yaml
read:
  - CONVENTIONS.md
```

See [examples/aider/](examples/aider/) for details.

## Universal Setup Script

For convenience, a setup script is provided:

```bash
# Setup all supported agents
./scripts/setup-agents.sh --agent all

# Setup specific agent
./scripts/setup-agents.sh --agent cursor
./scripts/setup-agents.sh --agent cline
```

## Environment Variables

Credentials are feature- and provider-specific, not global agent prerequisites.
Configure the selected LLM provider only for LLM-backed features. Anthropic is
the Rust provider's default; OpenAI, Ollama, openai-compatible and the local
`agent` provider have their own configuration requirements.

```bash
# Only for LLM calls using the Anthropic provider
export ANTHROPIC_API_KEY="<your-anthropic-key>"

# Optional: Data directory
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"

```

The current CLI/MCP path has no browser executor or CAPTCHA solver; web forms
use durable manual tasks. A CAPTCHA API key does not enable browser execution.

## Testing Integration

Verify your agent can access the skill:

1. **SKILL.md agents** (Hermes, Copilot, Codex, Cursor, Windsurf):
   ```bash
   # Ask your agent:
   "What skills are available?"
   "Help me remove my data from data brokers"
   ```

2. **Rule-based agents** (Continue, Cline, Aider):
   ```bash
   # The agent should reference Symaira EraseMe when relevant
   "I want to exercise my GDPR rights"
   "Help me opt out of data brokers"
   ```

## Troubleshooting

### SKILL.md not discovered

- Ensure the skill directory name matches the `name:` in frontmatter
- Check that the file is named exactly `SKILL.md`
- Reload skills: `/skills reload` (Copilot/Codex) or restart the agent

### Commands not found

- Install Symaira EraseMe: `brew install danieljustus/tap/symeraseme`
- Or download the matching static archive from GitHub Releases
- Verify: `symeraseme version`

### API key errors

- Check the selected LLM provider and its feature-specific configuration.
- Set `ANTHROPIC_API_KEY` only for Anthropic; the `agent` provider uses a local backend.
- Ensure any required provider environment variables reach the agent process.

## Contributing

To add support for a new agent:

1. Research the agent's skill/tool format
2. Create an example in `examples/<agent-name>/`
3. Update this `AGENTS.md`
4. Update `skills/SKILL.md` agent list
5. Submit a PR

## References

- [agentskills.io](https://agentskills.io) — Open skill standard
- [Claude Code Docs](https://docs.anthropic.com/en/docs/claude-code)
- [Hermes Docs](https://hermes-agent.nousresearch.com/docs/)
- [Codex Docs](https://developers.openai.com/codex/)
- [Cursor Docs](https://cursor.com/docs)
- [Windsurf Docs](https://docs.windsurf.com/)
- [Continue Docs](https://docs.continue.dev/)
- [Cline Docs](https://docs.cline.bot/)
- [Aider Docs](https://aider.chat/docs/)

## macOS App (`app/SymairaEraseMe/`)

- SwiftUI SPM executable (`SymairaEraseMe`). Build: `cd app/SymairaEraseMe && swift build`
  (local builds need `DEVELOPER_DIR` pointing at a full Xcode installation).
- Depends on the shared **symaira-appkit** package, pinned exact in
  `Package.swift`: SymairaTheme and SymairaToolKit.
- `ServerManager` launches the bundled or development `symeraseme mcp` Rust
  binary over HTTP; Homebrew/configured Binary Path is the fallback.
- Migration context: see `docs/go-test-port-classification.md` and
  `TROUBLESHOOTING.md`.