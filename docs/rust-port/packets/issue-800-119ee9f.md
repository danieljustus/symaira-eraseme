# Issue #800 implementation packet

- **Base SHA:** `119ee9f84fe7c9e1485d25ab10aac8582e98395c`
- **Worktree:** `/Users/daniel/Dev/Symaira Dev/symaira-eraseme/.worktrees/issue-800`
- **Branch:** `issue/800-manual-webform`
- **Timeout:** 20 minutes implementation; preserve partial work on timeout
- **Recovery point:** clean worktree at current `origin/main`; do not reset, rebase, push, open a PR, or touch the main checkout.

## Decision

Use the **explicit durable manual-task fallback**. Do not invent a new `symbrowse` CLI/MCP protocol during this rewrite. Evidence: `symbrowse` has no stable externally consumable dynamic formflow command/tool; the accepted Rust proposal explicitly puts new browser automation out of scope. Remove EraseMe's compile-time dependency on `github.com/danieljustus/symaira-browse` and retain only local, language-neutral web-form preview/result contracts.

## Required behavior

1. `run_web_form` dry-run remains a truthful side-effect-free preview.
2. Non-dry `run_web_form` never reports success without a real injected executor. The production no-executor path creates and returns a durable `manual_tasks` entry with `success=false`, `status=manual_action_required`, `reason=dynamic_form`, task id, broker id/name, URL, instructions and `dry_run=false`.
3. Campaign `execute` in CLI and MCP wires the same fallback runner. Existing `executeWebformRequest` must create `HUMAN_ACTION_REQUIRED` then `SEND_FAILED` and return the task id instead of failing with `ErrWebFormRunnerRequired`.
4. `auto_confirm` with a trusted extracted link and no clicker creates a durable manual task linked to the request and returns a manual-confirmation outcome with URL, task id and instructions. It must not claim a click/success. Dry-run stays side-effect-free.
5. Missing `symbrowse`/empty PATH must not change behavior, invoke a subprocess, or claim success. No real browser, broker, credential, or personal data in tests.
6. Preserve deterministic result/reason/evidence mapping for injected fake executors without importing sibling code. Apply bounded context cancellation/timeouts at any injected side-effect boundary.
7. Update README, TROUBLESHOOTING, MCP contract, skill docs, Go→Rust plan/matrix/task graph and dependency files truthfully. Set `DOM-000B=PASS` only with executable evidence; do not mark Phase 0 complete while #794 remains deferred.

## Allowed files

- `internal/campaign/webform.go` and focused tests
- `internal/campaign/execution.go`/batch tests only as required
- `internal/confirmation/**`, `internal/replies/**`
- `internal/mcp/contract_handler.go` and focused tests
- `cmd/symeraseme/real_commands.go` and focused tests
- `go.mod`, `go.sum`
- `README.md`, `TROUBLESHOOTING.md`, `docs/mcp-contract.md`, relevant `skills/**`
- migration plan/task graph/matrix and durable packet/review files

No unrelated refactors or feature work.

## Acceptance commands

- focused Go tests for campaign/confirmation/replies/MCP/CLI
- `go mod tidy`
- `make test`
- `make test-race`
- `make lint`
- `make vet`
- `make coverage`
- `make build`
- `git diff --check`
- confirm `go list -m all` and tracked Go imports contain no `symaira-browse`

## Required output

Follow TDD. Commit the smallest coherent solution locally and report exact SHA plus exact command outcomes. Do not push or create a PR.
