# Task 1.3 specification review

- Issue: #803
- Task: 1.3 — Add dual-language Make targets
- Reviewed SHA: `3e2b3ef42267ef4287ffbf7f7d8bbcdf7abceef5`
- Reviewer lane: specification
- Verdict: **PASS**

## Verification

- Existing Go targets remain present and functional.
- `build-go` and `build-rust` use separate output roots and remove stale
  binaries before rebuilding.
- `go-gate`, `rust-gate` and `parity` have explicit dependency graphs.
- `clean` removes generated Go/Rust/dist/coverage artifacts without touching
  committed parity cases or fixtures.
- Missing tool tests fail explicitly rather than returning fake success.
- GoReleaser snapshot validation and `release-dry-run` pass.
- `app-test` correctly reports the host capability boundary: only CommandLineTools
  are selected; a full Xcode installation is required.
- Diff/scope checks pass and no unrelated files changed.

## Local checks

`make fmt-check`, `make test`, `make lint`, `make vet`, `make build`,
`make build-go`, `make go-gate`, `make build-rust`, `make rust-gate`,
`make parity` and `make clean` passed. The app target remains intentionally
host-blocked as documented above.
