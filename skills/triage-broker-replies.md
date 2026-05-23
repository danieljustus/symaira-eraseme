# Triage Broker Replies

Guide an AI agent or user through daily inbox polling and reply classification.

## Prerequisites

- Email account configured with IMAP access
- Removal requests sent (at least a few days ago)
- LLM provider configured (set `OPENERASEME_LLM_PROVIDER` and provider-specific API key)

## Step 1: Poll the inbox

Fetch recent emails and match them against pending removal requests:

```bash
openeraseme poll-inbox \
  --host imap.gmail.com \
  --port 993 \
  --username jane@gmail.com \
  --password "app-password" \
  --since 3 \
  --campaign initial
```

### JSON output

```bash
openeraseme poll-inbox ... --output json
```

```json
{
  "total_fetched": 12,
  "total_matched": 2,
  "messages": [
    {
      "message_id": "<abc@broker.com>",
      "request_id": 1,
      "from_addr": "support@acxiom.com",
      "subject": "Re: Data Deletion Request",
      "body": "We have received your request..."
    }
  ]
}
```

## Step 2: Classify a reply

Use the LLM classifier to understand the broker's response:

```bash
# Using Anthropic (default)
openeraseme classify-reply 1

# Using OpenAI
openeraseme classify-reply 1 --provider openai --model gpt-4o

# Using local Ollama
openeraseme classify-reply 1 --provider ollama --model llama3.1
```

The classifier categorizes replies as:

| Classification | Meaning | Next action |
|----------------|---------|-------------|
| `CONFIRMATION` | Request acknowledged | No action needed; update status |
| `VERIFICATION_REQUIRED` | Need to verify identity | See [handle-action-required](handle-action-required.md) |
| `REJECTED` | Request denied | Generate rebuttal |
| `COMPLETED` | Data deleted | Mark as done |
| `REQUEST_MORE_INFO` | Need additional details | Provide requested info |
| `ESCALATED_TO_HUMAN` | Sent to human agent | Monitor for follow-up |
| `AUTO_CONFIRM` | Auto-confirmation link found | Run `auto-confirm` |

### JSON output

```bash
openeraseme classify-reply 1 --output json
```

```json
{
  "request_id": 1,
  "reply_id": 1,
  "classification": "VERIFICATION_REQUIRED",
  "event_type": "VERIFICATION_REQUESTED",
  "confidence": 0.95,
  "summary": "Broker requests identity verification via email link",
  "needs_human_review": false,
  "extracted_fields": {
    "verification_url": "https://acxiom.com/verify?token=abc"
  },
  "usage": {
    "cost": 0.001234,
    "input_tokens": 450,
    "output_tokens": 120
  }
}
```

## Step 3: View event history

```bash
openeraseme events show 1
openeraseme events show 1 --output json
```

## Daily triage workflow

Recommended daily schedule:

1. **Morning**: Run `poll-inbox` to fetch new replies
2. **Classify**: Run `classify-reply` for each unmatched reply
3. **Act**: Handle each classification:
   - `CONFIRMATION` / `COMPLETED` → Log and continue
   - `VERIFICATION_REQUIRED` → Run `auto-confirm` or handle manually
   - `REJECTED` → Run `generate-rebuttal`
   - `REQUEST_MORE_INFO` → Draft response
   - `AUTO_CONFIRM` → Run `auto-confirm`
4. **Evening**: Run `tick` for deadline tracking

## Best practices

1. **Poll daily**: Brokers typically respond within 1-5 business days.
2. **Check confidence**: Low-confidence classifications (<0.7) may need human review.
3. **Batch classifications**: Classify all unmatched replies in one session to save API costs.
4. **Save API keys**: Set `OPENERASEME_LLM_PROVIDER` and the provider-specific API key (e.g. `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`) in the environment to avoid passing `--provider` every time.

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `IMAP error` | Invalid credentials or server | Check email/password; use app-specific password |
| `No unclassified inbox reply found` | All replies already classified | Check `events show <id>` for existing classifications |
| `LLM provider not available` | Provider or API key not configured | Set `OPENERASEME_LLM_PROVIDER` and provider-specific API key |
| `No new messages found` | No recent emails | Increase `--since` to look further back |
