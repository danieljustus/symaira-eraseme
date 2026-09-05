# Issue #796 specification review — revision 1

- **reviewed_sha:** `daeb90067a988131d605679c6b9938da49981bb8`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC FAIL
- **commands:** full commit diff review; focused package tests; full `go test -count=1 ./...`; contract/config/encryption call-site inspection
- **results:** tests passed for the reviewed commit, but executable contract gaps remained

## Findings

1. A fresh process requesting plaintext mode could not decrypt an existing encrypted database because CLI and MCP bootstrapped the identity key only when `encrypt_db=true`. The revision must detect an encrypted canonical file and perform read-only key bootstrap before downgrade.
2. Issue #796 changed unrelated MCP bind defaults by reading `port` and `allow_remote` from config. Restore the frozen CLI defaults; transport configuration is outside this storage issue.
3. `config show` gained a new storage envelope/text shape. Restore the existing public output; CLI-009 has not been deliberately changed.
4. Registry resource loading was moved through the replacement config loader although #796 concerns storage. Preserve the existing `SYMERASEME_RESOURCES` behavior.
5. The replacement loader changed corekit-compatible `Reload` semantics by replacing the cached `Load` snapshot. `Reload` must return a fresh value without mutating the existing cache; `ResetCache` owns cache replacement.

## Required revision gate

Add a fresh-process encrypted→plaintext regression for both production openers, restore unrelated public behavior, preserve loader cache semantics, then rerun focused and full gates before re-review.
