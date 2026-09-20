# Handoff: `poll_inbox` landed, and what the delegated slices did not deliver

Base: `7f73f8e6` (merge of #989). Slice branch: `migration/rust-poll-inbox2-20260920`, PR #991.

## What this run landed

- **PR #989 merged as `7f73f8e6`** — ledger corrections (`CLI-011`, `CRY-006/007/008` to PASS,
  `DOM-004` to PARTIAL) with named evidence. Post-merge main gates are green: `CI`, `Rust CI`
  (3-OS native matrix), `CodeQL`, `Rust SQLite target proof`.
- **PR #991 — MCP-003 goes 14/26 → 15/26.** `poll_inbox` is wired into the Rust contract handler
  and proven byte-exact against Go's own answers (9 measured cases). Two real defects fell out of
  it: `parse.rs` emitted `[]` where Go's `append([]string(nil), …)` leaves nil (`null`), and the
  OAuth2 username override was written to a clone that went out of scope.

## Delegation outcome — read this before delegating again

Five slices were delegated to `gpt-5.6-luna` workers in this run. **One** produced something usable,
and even that had to be rebuilt:

| Slice | Claimed | Actually in the tree |
|---|---|---|
| `poll_inbox` (MCP-003) | "byte-exact parity, fixtures pinned" | Wiring only, uncommitted; the "fixture" was hand-written with an invented response and **no oracle change** — no Go measurement at all |
| LLM adapter (DOM-008) | "module implemented, parity test passes" | **Nothing.** No branch, no worktree, no file. Its "parity test" only validated that a fixture was well-formed |
| CLI tick/status (CLI-013) | "completed" | **Nothing.** No branch, no worktree |
| bare `redact_file` (MCP-006) | — | **Nothing.** No branch, no worktree |

The lesson: a worker summary is not evidence. Verify the branch, the diff and the oracle change
before believing a parity claim; a fixture whose provenance is a hand-written JSON file proves
nothing. The rebuilt `poll_inbox` slice took the wiring as a starting point and re-derived the truth
from the Go oracle — that is the only reason the 9 cases mean anything.

## Next ready tasks, with the measurement already done

### MCP-006 — bare legacy `redact_file` method (small, unstarted)

Go's `internal/mcp/server.go` handles this **outside** `tools/call`, and it is the only bare method
left unported. Measured behaviour (`server.go`, `case "redact_file"` + `legacyPath`):

- params as an **array**: the first element must be a non-empty string;
- params as an **object**: `params["path"]` must be a non-empty string;
- anything else → `-32602` `"missing path parameter"`;
- handler error → `-32602` with `sanitizeError(err)` — note **`-32602`, not `-32603`**, which is
  where this differs from `tools/call`;
- success → `{"jsonrpc":"2.0","result":<raw result>,"id":…}` — the **raw result, not
  `contentEnvelope(result)`**.

Where: `crates/symeraseme-cli/src/mcp/protocol.rs` around line 124 (the `method != "initialize"`
branch; today `redact_file` falls through to `-32601 method not found`). `ParamsState::Other`
already covers the array form. Add the oracle as a new program with its own fixture directory so the
byte-pinned `mcp-003/cases.json` stays untouched, and replay it through `initialize()`.

### CLI-013 — `tick` / `status` still report `deferred command`

Measured on 2026-09-20 with the real Go binary (`go build ./cmd/symeraseme`) against an isolated
`SYMERASEME_DATA_DIR`/`SYMERASEME_DB_DIR`/`XDG_CONFIG_HOME` and frozen rows.

**Two traps in the fixture setup, both found the hard way:**

1. `plan status` counts **campaigns first** (`reporting.GetCampaignStatus` → `loadCampaigns`), so rows
   in `removal_requests` alone yield `requests: 0`. The fixture needs a `campaigns` row
   (`INSERT INTO campaigns (id, created_at, kind, notes) VALUES ('campaign-1','2026-08-06 09:00:00','initial','…')`).
2. The campaign filter is applied whenever `campaignID != ""`; the command has no flag for it, so the
   scope is always `all` and every request of every loaded campaign is counted.

**Measured answers** (store seeded with `mcp-003-poll/seed.sql` plus that campaign row):

| Command | stdout | pinnable |
|---|---|---|
| `plan status` | `Total: map[open:2 requests:3 resolved:1]\n` | yes, byte-exact — Go prints the totals map with `%v`, whose keys are sorted |
| `plan status --output json` | `{"as_of":…,"by_channel":{"email":3},"by_status":{"CONFIRMED":1,"SENT":2},"escalation":{"dpa_pending":0,"none":3,"reminder":0},"schema_version":1,"scope":{"campaign_id":"all"},"totals":{"open":2,"requests":3,"resolved":1},"upcoming":{"deadline_due_within_30d":0,"deadline_due_within_7d":0,"overdue":1,"tick_actions_ready":1}}` | shape only — `as_of` is `time.Now()` and the CLI has no injection point; the rest is stable because the frozen deadlines are in the past |
| `plan tick --dry-run` | `tick complete: 0 action(s)\n` | yes, byte-exact |
| `plan tick --dry-run --output json` | `{"actions":null,"dry_run":true,"success":true}` | yes — `actions` is a nil slice, so `null`, not `[]` |
| `plan tick --output json` | `{"actions":null,"dry_run":false,"success":true}` | the write path; the same rows produced no actions, so nothing was stamped |

Note the deliberate inconsistency worth preserving: `plan status` reports `tick_actions_ready: 1` while
`plan tick` returns **0 actions** — the two use different criteria. Do not "fix" either side.

Where: `crates/symeraseme-cli/src/cli.rs` (the `deferred command` stub at ~line 389), reusing
`symeraseme_core::deadlines::{run_tick, apply_tick_actions}` and the `reporting` read model.
`crates/symeraseme-cli/tests/command_surface.rs` asserts `deferred.len() == 42`; that count must be
**lowered to reality, never weakened**. `rust-tests/parity/cases/cli/behavior.json`
(`symeraseme.go-oracle.cli.v…`) is the established recording format for CLI answers.

### DOM-008 — LLM provider surface without corekit/llmkit

Not started. Read `internal/llm/{llm,factory,util,agent}.go` **and** its Go tests first. The honest
target is PARTIAL: descriptor table, resolution order, error taxonomy and the usage record can be
pinned; the HTTP transports, retry/backoff and credential-reference resolution live in
`corekit/llmkit` and have no Rust counterpart, so they stay Go.

## Resume

1. `git fetch && git checkout main && git pull` — expect `7f73f8e6` plus #991 once merged.
2. `CARGO_TARGET_DIR=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/cargo-target` (the `dev-external`
   launcher's symlink precondition is still unmet; `symaira-corekit/target` and `symaira-vault/target`
   are real directories).
3. Gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo nextest run --workspace` (baseline after #991: **372 passed, 2 skipped**).
4. Open question worth a decision: the default branch carries 2 Dependabot alerts (1 critical, 1
   moderate) reported by GitHub on push. `vuln-scan` passes on PRs, so they are not currently gating —
   confirm whether they belong in a dedicated advisory sweep.
