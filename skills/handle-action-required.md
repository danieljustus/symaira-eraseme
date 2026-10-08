# Handle Action Required

Guide an AI agent or user through responding to broker verification requests,
rejections, and other action-required scenarios.

## Prerequisites

- [Reply classified](triage-broker-replies.md) as `VERIFICATION_REQUIRED`,
  `REJECTED`, or `AUTO_CONFIRM`
- The current Rust CLI/MCP path does not inject a browser executor, so a
  manual task is the default. Configure an LLM provider only for rebuttal
  generation.

## Scenario 1: Auto-confirmation links

Many brokers send a confirmation link. The default Rust CLI/MCP path has no
browser executor: `auto-confirm` creates a linked durable manual task with
`step=manual_confirmation_required` and does not click the link. A host that
explicitly injects a supported executor may perform a click; do not assume one
is available.

### Dry-run first

```bash
# Preview the trusted link; no click is attempted
symeraseme auto-confirm 1 --dry-run
```

### Default manual-task path

```bash
symeraseme auto-confirm 1
symeraseme manual-tasks list
symeraseme manual-tasks show <task-id>
```

The JSON response from the default path reports
`manual_confirmation_required`; it does not claim a successful click. Complete
the task only after the user has manually followed the link and verified the
result:

```bash
symeraseme manual-tasks complete <task-id> --notes "Confirmed manually"
```

`--headed` and `--screenshot-dir` are executor options; they do not add browser
execution to the default Rust CLI/MCP path.

## Scenario 2: Generate a rebuttal

When a broker rejects a removal request, generate a legal rebuttal:

```bash
symeraseme generate-rebuttal 1
```

Configure credentials through the selected LLM provider's environment or secure
store. There is no `--api-key` CLI flag.

The LLM analyzes the rejection reason and generates a targeted rebuttal
based on the appropriate legal framework (GDPR Article 17, CCPA, etc.).

### JSON output

```bash
symeraseme generate-rebuttal 1 --output json
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

### Review the draft

The rebuttal text is printed to stdout. Review it with the user. The default
Rust CLI does not send email; the user may copy the draft into their chosen mail
client or use a separately injected email sender.

## Scenario 3: Manual fallback for complex web forms

The current Rust CLI/MCP path does not inject a browser executor. A dry run
previews the registry form; a non-dry run creates a durable manual task with
`reason=dynamic_form` and does not open a browser or submit the form.

```bash
# Preview only
symeraseme run-web-form <broker-id> --dry-run

# Create and review the manual task
symeraseme run-web-form <broker-id>
symeraseme manual-tasks list
symeraseme manual-tasks show <task-id>
```

Mark the task complete only after the user has performed and confirmed the
manual action:

```bash
symeraseme manual-tasks complete <task-id> --notes "Completed opt-out manually"
```

## Best practices

1. **Confirmation links**: `auto-confirm` previews or creates the default manual task; it does not click links without an injected executor.
2. **Check confidence**: Rebuttals with `confidence < 0.7` or
   `needs_human_review: true` should be reviewed by the user.
3. **Manual tasks**: Always review the instructions with the user and offer
   to open the URL in their browser.
4. **Executor diagnostics**: Use `--headed` and `--screenshot-dir <path>` only
   with a host that explicitly injects a browser executor; the default CLI does
   not run a browser.

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `No unclassified inbox reply found` | Already classified or no reply | Check `events show <id>` |
| `manual_confirmation_required` | No browser executor is injected in the default Rust CLI/MCP path | Review the durable manual task and perform the action yourself; use an injected executor only when a host explicitly provides one |
| `Manual task not found` | Invalid task ID | Run `manual-tasks list` to find valid IDs |
| LLM provider authentication error | Credential/configuration missing for the selected provider | Configure that provider only if using rebuttal generation; the Rust provider defaults to Anthropic (`ANTHROPIC_API_KEY`) |
| `Could not find confirmation link` | No link in the reply | Check the reply body manually |
