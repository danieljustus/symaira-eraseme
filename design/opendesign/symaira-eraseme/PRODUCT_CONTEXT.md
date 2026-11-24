# Product Context — Symaira EraseMe

**Repository:** `/Users/daniel/Dev/Symaira Dev/symaira-eraseme`
**Product role:** CLI-first GDPR/CCPA/LGPD data-broker removal tool with a separate macOS graphical app.

## Product focus

EraseMe covers identity profile setup, broker registry, removal campaign planning, request sending, deadline tracking, inbox triage, escalation, re-scans, and evidence/report export. The core is local, encrypted, auditable, and scriptable.

## Design priorities

- Show the removal lifecycle distinctly: planned, sent, awaiting response, action required, completed, rejected, overdue, escalated.
- Make jurisdiction, deadline, broker, evidence, and next action easy to scan.
- Treat CAPTCHA, verification links, manual review, IMAP failure, and provider outage as honest states.
- Keep privacy and legal boundaries visible without making unsupported legal promises.
- Design the macOS app as a focused companion to the CLI, not as a cloud dashboard.

## Prototype boundaries

Use synthetic identities, broker names, and statuses only. Never put real PII, email contents, API keys, or account credentials into an artifact. Do not claim a removal succeeded without explicit synthetic evidence.