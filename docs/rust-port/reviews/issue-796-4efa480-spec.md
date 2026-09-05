# Issue #796 specification review — recovery hardening

- **reviewed_sha:** `4efa4807a61ba601e3f6039bdb501f3f6683870b`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC FAIL
- **commands:** exact follow-up diff; recovery/cleanup implementation and tests
- **results:** retry and cleanup hardening preserved data, but one crash-artifact prefix was omitted from stale scavenging

## Finding

`atomicWriteFile` creates `.symeraseme_write_*.tmp`, while `ScavengeStaleTemps` did not recognize that prefix. A crash could therefore leave a private transition file indefinitely.

## Required revision gate

Include `.symeraseme_write_` in stale artifact recognition and add a regression proving an old write temp is removed without deleting recent or unrelated files.
