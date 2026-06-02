# Hermes Integration

[Hermes](https://hermes-agent.nousresearch.com/) is an open-source AI agent by Nous Research with native SKILL.md support.

## Installation

### Option 1: Direct URL Install

```bash
hermes skills install https://raw.githubusercontent.com/danieljustus/Symaira-EraseMe/main/skills/SKILL.md
```

### Option 2: Manual Install

```bash
# Clone the repository
git clone https://github.com/danieljustus/Symaira-EraseMe.git
cd symaira-eraseme

# Copy to Hermes skills directory
mkdir -p ~/.hermes/skills/privacy-tools/symaira-eraseme
cp skills/SKILL.md ~/.hermes/skills/privacy-tools/symaira-eraseme/
cp -r skills/*.md ~/.hermes/skills/privacy-tools/symaira-eraseme/

# Verify installation
hermes skills list
```

### Option 3: External Directory

Add to `~/.hermes/config.yaml`:

```yaml
skills:
  external_dirs:
    - /path/to/symaira-eraseme/skills
```

## Usage

Once installed, Hermes will automatically discover the skill at session start.

### Invoke the skill

```
User: Help me remove my data from data brokers

Hermes: [Loads symaira-eraseme skill and guides through workflow]
```

### Progressive Disclosure

Hermes uses three levels of disclosure:
1. **Level 0**: Metadata only (~3k tokens) — loaded at session start
2. **Level 1**: Full SKILL.md — loaded when skill is invoked
3. **Level 2**: Reference files — loaded on demand

## Skill Structure

```
~/.hermes/skills/privacy-tools/symaira-eraseme/
├── SKILL.md                    # Main skill definition
├── setup-identity.md           # Sub-skill: Identity setup
├── plan-removal-campaign.md    # Sub-skill: Campaign planning
├── send-removal-batch.md       # Sub-skill: Sending requests
├── triage-broker-replies.md    # Sub-skill: Inbox triage
├── handle-action-required.md   # Sub-skill: Handling responses
├── daily-tick.md               # Sub-skill: Tick engine
└── re-scan-quarterly.md        # Sub-skill: Quarterly re-scan
```

## Environment Variables

Set these before running Hermes:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export SYMERASEME_DATA_DIR="$HOME/.symeraseme"
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skill not listed | Run `hermes skills reload` or restart Hermes |
| Commands not found | Ensure `symeraseme` is in PATH: `pip install symeraseme` |
| API key errors | Check `ANTHROPIC_API_KEY` is exported in environment |

## Documentation

- [Hermes Skills Docs](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)
- [Creating Skills](https://hermes-agent.nousresearch.com/docs/developer-guide/creating-skills)
- [agentskills.io Standard](https://agentskills.io)
