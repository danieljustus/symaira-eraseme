# Issue #796 specification review — revision 2

- **reviewed_sha:** `210ac2571c4381c4b264e852becf62a38718e598`
- **reviewer_lane:** Hermes native targeted reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC FAIL
- **commands:** exact base-to-commit diff; focused package tests; storage/config/encryption inspection
- **results:** all five revision-1 findings were fixed and focused tests passed; one documentation mismatch remained

## Finding

`docs/event-store.md` still named the old `$TMPDIR`/`/dev/shm` decrypted-copy location and did not explicitly document plaintext↔encrypted mode transitions, WAL checkpointing, atomic replacement, key requirements, and retained recovery state.

## Required revision gate

Align the pinned event-store contract with `DefaultEncryptedTempDir` and the implemented safe transition lifecycle, then re-review the exact revision.
