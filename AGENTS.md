# AI Agent Integration Guide

**Symaira EraseMe** supports all major AI coding agents through standardized
skill formats and adapter files.

## Ecosystem Guidance

- Before changing cross-tool integrations, shared conventions, or product
  boundaries, read `../docs/00-MASTERPLAN.md` and `../ECOSYSTEM.md`.
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

These agents auto-discover from `.agents/skills/` (already configured in this repo).

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
**Status**: ✅ Already configured (symlink exists)

```bash
cd /path/to/symaira-eraseme
# Already configured:
ls -la .claude/skills/
# symeraseme -> ../../skills/
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
**Status**: ✅ Auto-discovered from `.agents/skills/`

```bash
# Already configured in this repo:
ls -la .agents/skills/
# symaira-eraseme -> ../../skills/
```

Verify:
```bash
copilot /skills reload
copilot /skills info symaira-eraseme
```

### Codex CLI

**Format**: `SKILL.md`  
**Path**: `.agents/skills/` or `~/.codex/skills/`  
**Status**: ✅ Auto-discovered from `.agents/skills/`

```bash
# Already configured in this repo
codex /skills reload
codex /skills info symaira-eraseme
```

See [examples/codex/](examples/codex/) for optional metadata file.

### Cursor

**Format**: `SKILL.md` (skills) + `.mdc` (rules)  
**Path**: `.cursor/skills/` or `.agents/skills/`  
**Status**: ✅ Auto-discovered from `.agents/skills/`

Optional: Add rules for enhanced context:
```bash
mkdir -p .cursor/rules
cp examples/cursor/symaira-eraseme.mdc .cursor/rules/
```

See [examples/cursor/](examples/cursor/) for details.

### Windsurf

**Format**: `SKILL.md` (skills) + `.md` (rules)  
**Path**: `.windsurf/skills/` or `.agents/skills/`  
**Status**: ✅ Auto-discovered from `.agents/skills/`

Optional: Add rules and workflows:
```bash
mkdir -p .windsurf/rules .windsurf/workflows
cp examples/windsurf/symaira-eraseme.md .windsurf/rules/
cp examples/windsurf/remove-data.md .windsurf/workflows/
```

See [examples/windsurf/](examples/windsurf/) for details.

### Continue

**Format**: `.md` rules  
**Path**: `.continue/rules/`  
**Status**: Manual setup required

```bash
mkdir -p .continue/rules
cp examples/continue/symaira-eraseme.md .continue/rules/
```

Optional: Create `.continuerc.json` for project config.

See [examples/continue/](examples/continue/) for details.

### Cline

**Format**: `.md` rules  
**Path**: `.clinerules/`  
**Status**: Manual setup required

```bash
mkdir -p .clinerules
cp examples/cline/00-symaira-eraseme.md .clinerules/
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
./scripts/setup-agents.sh

# Setup specific agent
./scripts/setup-agents.sh --agent cursor
./scripts/setup-agents.sh --agent cline
```

## Environment Variables

All agents require these environment variables:

```bash
# Required for LLM triage
export ANTHROPIC_API_KEY="sk-ant-..."

# Optional: Data directory
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"

# Optional: CAPTCHA solving
export CAPSOLVER_API_KEY="CAP-..."
```

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

- Install Symaira EraseMe: `pip install symeraseme`
- Or from source: `uv sync && uv pip install -e .`
- Verify: `symeraseme --version`

### API key errors

- Set `ANTHROPIC_API_KEY` in your environment
- For Codex/Copilot: ensure env vars are passed to the agent

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

- SwiftUI SPM executable (formerly `SymairaDashboard`; renamed 2026-07 —
  EraseMe stays a STANDALONE consumer app per the ecosystem GUI strategy,
  it is not a hub module). Build: `cd app/SymairaEraseMe && swift build`
  (local builds need `DEVELOPER_DIR` pointing at Xcode).
- Depends on the shared **symaira-appkit** package, pinned exact (`0.1.0`)
  in `Package.swift`: SymairaTheme (shared brand tokens in
  `Theme/BrandColors.swift`; EraseMe-specific status colors, card backings
  and the padded GlassCard stay local) and SymairaToolKit (binary discovery
  in `Services/ServerManager.swift`).
- `ServerManager` is a long-running daemon supervisor (spawns
  `symeraseme serve`, with uv/python fallbacks) and stays app-local — it is
  the second requirements donor for a future SymairaDaemonKit (appkit v0.2).
- Migration context: see `../docs/symaira-appkit-migration.md` (Welle 3).
