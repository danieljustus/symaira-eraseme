# Go-source retirement candidate: native acceptance

## Source and scope

Candidate `3b8916b827768ab3500c0804d1cbf749cc22ad23` on
[PR #1232](https://github.com/danieljustus/symaira-eraseme/pull/1232) removes the
tracked Go implementation, module files and Go-toolchain execution. The readback
of `git ls-files '*.go' go.mod go.sum go.work go.work.sum` is empty. Active
workflows contain no Go installer, GoReleaser, or Go build/test/run/install/vet
invocation. The historical required check name **Go CI** is retained, not a Go
SDK dependency. Genuine frozen observations and historical release binaries
remain evidence, not a bundled fallback or production Go runtime.

The native Rust dispatch checked out this exact candidate. Pull-request jobs
checked out synthetic merge `7c52fbbe0ecabfcd1ea382d8cd1dbcdcb3115d0c`;
the complete Git trees were compared and are equal. Preserve those measured
identities. This is neither post-merge CI nor a newly published release.

All 20 candidate workflow runs were read back as successful. Detailed
readback below distinguishes actual execution from workflow conclusions.

## Native tests and Windows timeout correction

[Native Rust run 38468909364, attempt 1](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468909364)
completed all six native hosts. The full test/doctest step's raw summaries show:

| Target | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| macOS arm64 | 714 | 0 | 6 |
| macOS amd64 | 714 | 0 | 6 |
| Linux amd64 | 715 | 0 | 6 |
| Linux arm64 | 715 | 0 | 6 |
| Windows amd64 | 656 | 0 | 3 |
| Windows arm64 | 656 | 0 | 3 |

These are platform-specific test executions, not unique cross-platform cases.
Ignored entries are not counted as executed. Original failures are preserved:
run `38461554159` on the preceding candidate exhausted the identity launcher's
24-minute bound while running the whole Windows ARM64 workspace as one child.

The correction discovers all four packages from Cargo metadata and invokes the
unchanged native identity verifier once per package, then once for whole-workspace
doctests. Both Windows logs contain all four package markers and all five
successful parent/child identity checks. No deadline, fixture, comparison, or
identity check was weakened. Every package failure still fails the step.

## Artifact and application readback

- [Backup/restore run 38468916673](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468916673):
  all eight real cases on each of six native targets, 48 case executions.
  Artifact ZIP sizes/digests, ten committed harness inputs, retained binary and
  raw-stream hashes, schema `1 -> 2 -> 1`, exact restored state and rejected
  incomplete restore were independently checked.
- [Copied-store run 38468916925](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468916925):
  six plain and six encrypted cases per target, 72 executions and 144 verified
  stdout/stderr streams across six targets. Rust/Go readbacks match byte-for-byte.
  Every Rust input matches its actual same-run packed binary. The separately
  retained Go input is the official historical `v0.13.0` binary, not a Go SDK
  build or bundled rollback. No production data or production cutover is implied.
- [Archive run 38468916493](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468916493):
  the existing archive/SBOM validators accepted all six real Rust-only archives,
  their checksums, embedded dependency inventories and twelve sidecars, bound
  to the measured PR merge source. These are CI artifacts, not public assets.
- [Swift run 38468916722](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468916722):
  42 native XCTest cases passed on each Mac architecture, including bundled
  and explicit Rust launch, authentication, tool discovery and shutdown.
  Packaging-control logs explicitly use mocked signing. They are **not** proof
  of Developer ID signing, notarization, Gatekeeper or a mounted release DMG.

## Hardening and remaining acceptance

[Hardening run 38468916888](https://github.com/danieljustus/symaira-eraseme/actions/runs/38468916888)
executed the Miri contract test successfully and completed all six 120-second
fuzz targets. Schedule/dispatch-only mutation jobs were skipped, not represented
as executed. Both CodeQL analyses uploaded results in run `38468916848`.
Critical Rust line coverage in PR run `38468916659` is **95.22% (6476/6801)**,
above its 90% gate, recomputed from the retained raw log.

CUT-005 remains **OPEN**, with source retirement implemented on this candidate.
The final documentation/helper classification, protected `AGENTS.md` correction,
independent acceptance review and normal merge are not inferred from green CI.
A fresh release through the release process remains required by #1133, including
its applicable signing, notarization, mounted-DMG and distribution evidence.
Historical immutable `v0.13.0` and `v0.14.1` assets remain unchanged. The observation
period is explicitly waived, never represented as seven observed days.

Raw logs, original artifact ZIPs and machine-readable readback receipts are
retained in the coordinator evidence directory `ci-3b8916b8-acceptance`.
The linked GitHub runs identify the original evidence producers. No local path,
workflow conclusion or mock signing result substitutes for the remaining gates.
