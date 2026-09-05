# Issue #800 specification review — security revision

- **reviewed_sha:** `bdb6d3668672708ee1da185e9d7801561118adf0`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC FAIL
- **commands:** exact `f35a8c3..bdb6d36` diff; focused tests; persistence/output inspection
- **results:** web-form evidence was hardened, but successful/error confirmation results still exposed raw clicked URLs and screenshots and persisted them in `CONFIRMATION_LINK_CLICKED`

## Required revision gate

For actual click attempts, replace raw URL/screenshots in returned results and events with fixed allowlisted host, SHA-256 digests and byte counts. Preserve the validated URL only for dry-run/manual fallback where the user must act. Add a focused regression and rerun gates.
