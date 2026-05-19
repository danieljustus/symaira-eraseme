# OpenClaw Integration

This example shows how to configure OpenClaw to orchestrate data broker
removals using OpenEraseMe.

## Prerequisites

- [OpenClaw](https://github.com/openclaw/openclaw) installed
- OpenEraseMe installed (`uv sync`)
- Python 3.11+

## Setup

### 1. Create the OpenClaw skill

OpenClaw uses a YAML-based skill format. Create `~/.config/openclaw/skills/openeraseme.yaml`:

```yaml
name: openeraseme
description: >
  Automate data broker removals via the OpenEraseMe CLI.
  Supports GDPR/CCPA opt-out campaigns with email, web forms,
  inbox triage, and lifecycle management.

commands:
  # Identity
  init_profile:
    description: Create encrypted identity profile
    command: openeraseme init-profile
    inputs:
      full_name: { type: string, prompt: "Full name" }
      email: { type: string, prompt: "Email address" }

  show_profile:
    description: Display current identity profile
    command: openeraseme show-profile
    output_format: text

  # Planning
  plan_create:
    description: Plan a removal campaign
    command: >
      openeraseme plan create
      --campaign "{{ campaign_id }}"
      {% if jurisdiction %}--jurisdiction {{ jurisdiction }}{% endif %}
      {% if max %}--max {{ max }}{% endif %}
    inputs:
      campaign_id: { type: string, prompt: "Campaign ID" }
      jurisdiction: { type: string, optional: true, prompt: "Jurisdiction (GDPR/CCPA)" }
      max: { type: integer, optional: true, default: 30 }
    output_format: json

  plan_show:
    description: Show the current plan
    command: openeraseme plan show
    output_format: json

  # Execution
  execute:
    description: Send removal requests
    command: >
      openeraseme execute
      --campaign "{{ campaign_id }}"
      --batch-size {{ batch_size }}
      {% if dry_run %}--dry-run{% endif %}
      {% if consent_token %}--consent {{ consent_token }}{% endif %}
    inputs:
      campaign_id: { type: string, prompt: "Campaign ID" }
      batch_size: { type: integer, default: 5 }
      dry_run: { type: boolean, default: true }
      consent_token: { type: string, optional: true }
    output_format: json
    consent_required: true

  # Triage
  poll_inbox:
    description: Poll inbox for broker replies
    command: >
      openeraseme poll-inbox
      --username {{ username }}
      --password "{{ password }}"
      --since {{ since_days }}
      --host {{ host }}
    inputs:
      username: { type: string, prompt: "IMAP username" }
      password: { type: string, secret: true, prompt: "IMAP password" }
      host: { type: string, default: "imap.gmail.com" }
      since_days: { type: integer, default: 3 }
    output_format: json

  classify_reply:
    description: Classify a broker reply
    command: >
      openeraseme classify-reply {{ request_id }}
      {% if api_key %}--api-key {{ api_key }}{% endif %}
    inputs:
      request_id: { type: integer, prompt: "Request ID" }
      api_key: { type: string, secret: true, optional: true }
    output_format: json

  # Lifecycle
  tick:
    description: Run tick engine
    command: >
      openeraseme tick
      {% if dry_run %}--dry-run{% endif %}
    inputs:
      dry_run: { type: boolean, default: true }
    output_format: json

  # Actions
  auto_confirm:
    description: Auto-click confirmation link
    command: >
      openeraseme auto-confirm {{ request_id }}
      {% if dry_run %}--dry-run{% endif %}
    inputs:
      request_id: { type: integer, prompt: "Request ID" }
      dry_run: { type: boolean, default: true }
    output_format: json

  generate_rebuttal:
    description: Generate a rebuttal for a rejection
    command: >
      openeraseme generate-rebuttal {{ request_id }}
      {% if api_key %}--api-key {{ api_key }}{% endif %}
    inputs:
      request_id: { type: integer, prompt: "Request ID" }
      api_key: { type: string, secret: true, optional: true }
    output_format: json

  # Tokens
  grant:
    description: Issue a consent token
    command: openeraseme grant {{ command }} --ttl {{ ttl }}
    inputs:
      command: { type: string, default: "execute" }
      ttl: { type: integer, default: 3600 }
    output_format: text
```

### 2. Load the skill in OpenClaw

```bash
openclaw skill load openeraseme
```

### 3. Verify

```bash
openclaw skill list
# Should show: openeraseme (Automate data broker removals...)
```

## Example workflow

```bash
# 1. Initialize identity
openclaw run openeraseme.init_profile

# 2. Plan a campaign
openclaw run openeraseme.plan_create \
  --inputs '{"campaign_id": "initial", "max": 5}'

# 3. Review the plan
openclaw run openeraseme.plan_show

# 4. Dry-run execution
openclaw run openeraseme.execute \
  --inputs '{"campaign_id": "initial", "dry_run": true}'

# 5. Real execution (requires consent)
openclaw run openeraseme.grant \
  --inputs '{"command": "execute", "ttl": 3600}'

openclaw run openeraseme.execute \
  --inputs '{
    "campaign_id": "initial",
    "batch_size": 5,
    "consent_token": "<token from grant>"
  }'

# 6. Daily triage
openclaw run openeraseme.poll_inbox \
  --inputs '{"username": "jane@gmail.com", "password": "..."}'

openclaw run openeraseme.classify_reply \
  --inputs '{"request_id": 1}'

# 7. Handle actions
openclaw run openeraseme.auto_confirm \
  --inputs '{"request_id": 1}'

openclaw run openeraseme.generate_rebuttal \
  --inputs '{"request_id": 2, "api_key": "sk-ant-..."}'

# 8. Daily tick
openclaw run openeraseme.tick \
  --inputs '{"dry_run": true}'
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Skill not found | Check the YAML path is in OpenClaw's skill directory |
| JSON parsing errors | Use `--output json` for structured output |
| Consent required | Issue a token via `grant` before destructive operations |
| Variable interpolation | Use `{{ variable_name }}` syntax in YAML commands |
