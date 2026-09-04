# Issue 799 specification and security review

- **reviewed_sha:** `3b3490a3ce06b4d9c63b7185827f13b009377642`
- **reviewer_lane:** Hermes native reviewer
- **verdict:** SPEC PASS
- **commands:** focused IMAP/MCP/CLI/event-store tests, protocol transcript tests, focused race tests
- **evidence:** production adapter is wired; implicit TLS and STARTTLS verify certificates; cleartext auth requires explicit test-only configuration; raw/encoded secrets are redacted; oldest pending UID windows and explicit missing/malformed errors prevent HWM skips; durable HWM follows reply persistence.
