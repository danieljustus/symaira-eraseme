# Issue #796 specification review — post-rebase

- **reviewed_sha:** `d549e74f590e6df0590b9416d22ec34b73d08b9b`
- **base_sha:** `279b468c04fc69c9baf764be8497823b8f2f8722`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** SPEC PASS
- **commands:** exact base-to-head diff; focused storage/config/event-store/MCP inspection; focused Go tests
- **results:** rebase preserved #824 XOAUTH2 configuration and arguments while retaining joined encrypted-store close errors; all #796 storage, transition, restart, checkpoint, recovery, and documentation criteria remain satisfied

## Evidence

- `internal/mcp/poll_inbox.go` retains `LoadIMAPConfigWithOptions`, OAuth2 overrides, named returns, and `errors.Join(runErr, store.Close())`.
- CLI and MCP production openers resolve persistent storage, detect encrypted input, bootstrap the existing key read-only, and call `OpenConfigured`.
- The event-store contract documents both mode transitions, WAL checkpointing, atomic replacement, key requirements, private cache paths, and failure recovery.
- Focused tests passed with no files modified by the reviewer.
