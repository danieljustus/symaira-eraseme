# Issue #800 specification review — revision 1

- **reviewed_sha:** `0d5c3a86e8b89081c66dd020392ac270c81ace5f`
- **base_sha:** `119ee9f84fe7c9e1485d25ab10aac8582e98395c`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC FAIL
- **commands:** exact diff; focused and race tests; dependency/import and CLI/MCP surface inspection
- **results:** core manual fallback worked, but honest CLI output and bounded executor behavior were incomplete

## Findings

1. Text-mode `run-web-form` and `auto-confirm` printed `success` unconditionally even when the result was `success=false` and manual action was required.
2. Injected form and confirmation side-effect boundaries were unbounded when the registry omitted a timeout.
3. Dry-run returned resolved form-field values, unnecessarily exposing identity data and changing the prior preview surface.
4. Direct injected executor failures returned a manual-looking envelope but did not persist the documented pending manual task when a store was available.
5. Missing-`symbrowse` behavior lacked an explicit empty-`PATH` regression.

## Required revision gate

Render honest text outcomes; apply bounded default deadlines; keep dry-run free of resolved field values; persist failed executor outcomes; prove empty-PATH behavior; rerun focused and full gates.
