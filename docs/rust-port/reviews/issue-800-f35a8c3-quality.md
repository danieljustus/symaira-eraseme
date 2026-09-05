# Issue #800 quality/security review — revision 1

- **reviewed_sha:** `f35a8c3792e944f36c879e870fa9706e46e6b7a4`
- **base_sha:** `119ee9f84fe7c9e1485d25ab10aac8582e98395c`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** REQUEST_CHANGES
- **commands:** exact diff; focused race tests; event/manual-task/confirmation/CLI/MCP inspection

## Findings

1. Raw page text, screenshots, form fields, local paths and profile values could enter command responses, event payloads and manual-task records.
2. Sender-controlled email domains expanded the confirmation allowlist, and HTTP links were accepted.
3. A nil successful FormExecutor response could panic.
4. Manual-task row/event writes and completion/note writes were non-atomic; event errors were swallowed.
5. Repeated fallback attempts created duplicate pending tasks.
6. Manual and unsuccessful CLI outcomes still returned exit status zero.
7. Snapshot filenames used second-resolution timestamps and could collide.

## Required revision gate

Redact or summarize untrusted evidence; use a fixed HTTPS allowlist; handle nil results; make task/event transitions atomic; deduplicate pending fallbacks; return nonzero CLI status; use collision-resistant snapshot names; add focused regressions and rerun full gates.
