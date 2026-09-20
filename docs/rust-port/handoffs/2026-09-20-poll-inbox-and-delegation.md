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

Not started; no measurement taken yet. The Go reference is `cmd/symeraseme/{command_surface,real_commands,extra_commands}.go`,
and the Rust cores (`symeraseme_core::deadlines::{run_tick, apply_tick_actions}`, campaign/reporting
read models) already exist. `crates/symeraseme-cli/tests/command_surface.rs` asserts a deferred count
that must be **lowered to reality, never weakened**. Note the existing CLI golden fixture
`rust-tests/parity/cases/cli/behavior.json` (`symeraseme.go-oracle.cli.v…`) is the established
recording format for CLI answers.

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
