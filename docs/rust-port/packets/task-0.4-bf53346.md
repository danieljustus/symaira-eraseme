# Task 0.4 context packet — complete corrected Go oracle

- **base_sha:** `bf53346eec234929bedf0314b99e3da85dbb991b`
- **task:** `0.4`
- **phase_issue:** `#802`
- **oracle_commit:** `bf53346eec234929bedf0314b99e3da85dbb991b`
- **predecessors:** Task 0.2 and blocker fixes #795–#800, #816–#817, #825 are merged.
- **deferred external gate:** #794 code hardening merged in #820; credentialed signed/notarized RC proof remains required before Phase 9, not before fixture generation.

## Required outputs

1. `scripts/generate-go-oracle-fixtures.sh`
2. Deterministic cases under `rust-tests/parity/cases/{cli,mcp,http,filesystem}/`
3. `rust-tests/parity/fixtures/README.md`
4. Generated fixtures reproduced byte-for-byte by two consecutive runs from the pinned oracle commit.

## Contract

- Enumerate the complete Cobra tree recursively, including hidden commands, aliases, positional forms, flags, defaults and ordering.
- Capture CLI help/success/parse failure/missing argument/unknown flag with raw stdout, stderr and exit status.
- Capture raw MCP initialize/list/call/batch/notification/error cases.
- Capture HTTP method/auth/origin/body-limit cases using local fake servers only.
- Capture filesystem side effects in isolated roots.
- Force isolated HOME/XDG/TMPDIR, UTC and fixed locale; clear credential/provider variables; never read the developer profile, keychain or real database.
- Mark each genuinely nondeterministic field explicitly; do not broadly sort, trim, rewrite stderr or normalize JSON.
- Keep the standalone Go source and current Go tests unchanged.

## Verification gate

- Generation run 1 succeeds.
- Generation run 2 succeeds.
- `git diff --exit-code` after run 2 proves byte-identical regeneration.
- Fixture schema/README explain raw bytes, side-effect manifests and narrow nondeterminism markers.
- Existing `make test`, `make lint`, `make vet`, and `make build` remain green.
