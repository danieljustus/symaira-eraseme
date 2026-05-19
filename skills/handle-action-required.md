# Handle Action Required

Guide an AI agent or user through responding to broker verification requests,
rejections, and other action-required scenarios.

## Prerequisites

- [Reply classified](triage-broker-replies.md) as `VERIFICATION_REQUIRED`,
  `REJECTED`, or `AUTO_CONFIRM`
- `ANTHROPIC_API_KEY` set (for rebuttal generation)
- Playwright installed (for auto-confirmation links)

## Scenario 1: Auto-confirmation links

Many brokers send a confirmation link that needs to be clicked.

```bash
openeraseme auto-confirm 1
```

### Dry-run first

```bash
openeraseme auto-confirm 1 --dry-run
# Output: [DRY RUN] Would click: https://broker.com/verify?token=abc
```

### Headless vs headed mode

```bash
# Default: headless (no visible browser)
openeraseme auto-confirm 1

# Show browser for debugging
openeraseme auto-confirm 1 --headed
```

### JSON output

```bash
openeraseme auto-confirm 1 --output json
```

```json
{
  "request_id": 1,
  "success": true,
  "step": "click_confirm",
  "clicked_url": "https://broker.com/verify?token=abc",
  "error": null,
  "dry_run": false,
  "screenshot_before": null,
  "screenshot_after": "/tmp/screenshots/confirm_1_after.png"
}
```

## Scenario 2: Generate a rebuttal

When a broker rejects a removal request, generate a legal rebuttal:

```bash
openeraseme generate-rebuttal 1 --api-key "$ANTHROPIC_API_KEY"
```

The LLM analyzes the rejection reason and generates a targeted rebuttal
based on the appropriate legal framework (GDPR Article 17, CCPA, etc.).

### JSON output

```bash
openeraseme generate-rebuttal 1 --output json
```

```json
{
  "request_id": 1,
  "template_name": "gdpr-art17.rejection-rebuttal.md.j2",
  "label": "GDPR Article 17(1)(a) Rebuttal",
  "jurisdiction": "GDPR",
  "rejection_classification": "data_retention_claim",
  "confidence": 0.88,
  "needs_human_review": false,
  "llm_used": "claude-3-5-sonnet-latest",
  "rebuttal_body": "Dear Acxiom,\n\nWe write in response to your refusal...",
  "usage": {
    "cost": 0.002345,
    "input_tokens": 890,
    "output_tokens": 310
  }
}
```

### Review before sending

The rebuttal text is printed to stdout. Review it with the user, then send
via email using the configured email adapter.

## Scenario 3: Manual fallback for complex web forms

Some web forms are too complex for automated handling. Use the manual
fallback system:

```bash
# List pending manual tasks
openeraseme manual-tasks list

# Show details of a specific task
openeraseme manual-tasks show 1

# Mark as completed after manual action
openeraseme manual-tasks complete 1 --notes "Completed opt-out via manual browser session"
```

### JSON output

```bash
openeraseme manual-tasks list --output json
```

```json
[
  {
    "id": 1,
    "broker_name": "Spokeo",
    "broker_id": "spokeo",
    "form_url": "https://spokeo.com/opt-out",
    "reason": "multi_step_form_with_captcha",
    "status": "pending",
    "created_at": "2026-05-19T10:00:00",
    "instructions": "Navigate to URL and fill in the opt-out form..."
  }
]
```

## Best practices

1. **Auto-confirm first**: Try `auto-confirm` before any manual handling.
2. **Check confidence**: Rebuttals with `confidence < 0.7` or
   `needs_human_review: true` should be reviewed by the user.
3. **Manual tasks**: Always review the instructions with the user and offer
   to open the URL in their browser.
4. **Save screenshots**: Use `--screenshots /tmp/oe-screenshots` with
   `auto-confirm` and `run-web-form` for debugging.

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `No unclassified inbox reply found` | Already classified or no reply | Check `events show <id>` |
| `Failed: ...` | Browser automation error | Retry with `--headed` to debug |
| `Manual task not found` | Invalid task ID | Run `manual-tasks list` to find valid IDs |
| `Anthropic API not available` | API key missing | Set `ANTHROPIC_API_KEY` |
| `Could not find confirmation link` | No link in the reply | Check the reply body manually |
