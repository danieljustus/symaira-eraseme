# Task 0.4 specification review — revision 1

- **reviewed_sha:** `fbd768562e0541ce99402548aa18a3a02acd7139`
- **base_sha:** `bf53346eec234929bedf0314b99e3da85dbb991b`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`) plus coordinator integration check
- **verdict:** SPEC FAIL
- **commands:** two generator runs with recursive diff; fixture-count parsing; source and contract-matrix inspection

## Blocking findings retained at this revision

1. The CLI corpus lacks root `--version`, unknown-command behavior, and operational cases for many matrix rows.
2. Commands guarded only by `MaximumNArgs` are mislabeled as missing-argument parser failures.
3. Implicit Cobra help/version flags are absent from serialized metadata.
4. CLI cases share one runtime root instead of resetting state per case.
5. MCP protocol cases omit ID-type/null/invalid-ID, broader malformed params, multi-frame/truncation, and shutdown behavior.
6. HTTP cases omit content-type and host/remote-policy matrices.
7. Filesystem coverage is limited to initial MCP token creation and does not freeze profile/consent, scheduling, reporting/manual-task, migration, or token-rotation effects.

## Findings already corrected during integration

- The matrix now pins `bf53346…`.
- The generator now uses a private empty executable search path rather than inheriting host `PATH`.
- The task graph records 0.3 complete and 0.4 active; #794 is an explicit Phase 9 external gate.
