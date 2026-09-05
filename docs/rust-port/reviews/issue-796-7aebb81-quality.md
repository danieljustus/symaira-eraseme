# Issue #796 quality/security review — revision 1

- **reviewed_sha:** `7aebb816071b29cbe96de8fc6e6572f554ce833a`
- **base_sha:** `279b468c04fc69c9baf764be8497823b8f2f8722`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** REQUEST_CHANGES
- **commands:** exact diff review; focused race tests for eventstore/config/MCP/CLI; lock, atomic replacement, recovery and cross-platform inspection
- **results:** focused race tests passed, but a critical cross-process data-loss race remained

## Critical issue

Plain `OpenConfigured(..., false)` released/no longer held the sidecar database lock before returning, while `OpenEncrypted` could acquire that lock, checkpoint, and replace the same canonical database. A concurrent plaintext writer that did not hold the lock could commit WAL frames during the snapshot/replacement window, omitting those writes from the encrypted replacement.

## Required revision gate

Plain production stores must hold the same sidecar lock for their full writable lifetime, release it only after SQLite closes, and prove that a mode transition is blocked until the writer closes and that writes made before the successful transition survive.
