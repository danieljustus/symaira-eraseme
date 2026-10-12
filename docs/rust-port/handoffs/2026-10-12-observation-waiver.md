# Stable observation waiver and Go-retirement authorization

## Maintainer decision (2026-10-12)

The maintainer explicitly waived the remaining seven-day stable observation
period and authorized complete Go-source, module and toolchain retirement once
the other technical acceptance requirements are verified. The release and
retirement metadata changes to the protected agent guide were separately approved.

- [CUT-004 observation waiver](https://github.com/danieljustus/symaira-eraseme/issues/1132#issuecomment-6123976726)
- [CUT-005 explicit retirement approval](https://github.com/danieljustus/symaira-eraseme/issues/1133#issuecomment-6123977207)
- [Protected metadata authorization](https://github.com/danieljustus/symaira-eraseme/issues/1214#issuecomment-6123977604)

This is an explicit risk acceptance, **not seven completed days of observation**.
There is no claim of continuous production usage, no invented daily observation
record and no inference that elapsed time proves freedom from defects. The
original observation start and earliest-review timestamps remain historical
facts. The maintainer accepts the reduced opportunity to discover delayed
parity, data and security regressions before source retirement.

## Unchanged technical boundaries

The [published-candidate reconciliation](2026-10-11-published-rust-only-reconciliation.md)
remains the source-bound release/native evidence record. Its original failures,
limited source-equivalence scope and later corrections are preserved. The stable,
immutable `v0.14.1` release, release ID `409043734`, source
`587e9e0e3aaa4d4bbacdd28a7087d17ac2748548`, and historical `v0.13.0` assets are not
rewritten by this decision.

Signing/notarization, literal mounted-DMG evidence, native platform execution,
backup/copied-store compatibility, frozen genuine Go captures, provenance,
security and independent review requirements are not waived. A later changed
candidate needs its own applicable acceptance. Retained Go switchback evidence
proves the documented development oracle's compatibility, not compatibility
with an arbitrary older published Go binary.

## Execution status

CUT-004 retained technical evidence was revalidated at public main
`22615ae67325c581fbe60cb0f2d3d2b2139cf900` on 2026-10-12. The release API still
reports stable/immutable `v0.14.1`; all 21 retained public asset files were
rehash-checked against its unchanged asset identities, sizes and SHA-256 digests.
The retained mount probe, native report and manifest match their recorded hashes;
the current public Homebrew Formula matches the native-tested Formula bytes.
Original native/Homebrew runs `38083487544` and `38085163720` at
`89a0fbcdef745714950ca1994ceb11c0e8b832ab`, and frozen-reader run `38059031343`
at `5850fa0d3a08c2f849748713d3c9fe5238798c25`, still report success;
current-main Rust CI `38167547029` and native
target proof `38167547032` also report success. The full tracked diff from the
retained evidence base `89a0fbcd` to public main contains only three documents.
No open issue carries `parity-regression` in the complete intake.

The six existing Python publication/archive/privacy control suites passed
21 tests normally and the same 21 under optimization. These are fresh control
executions, not fresh native runs, public downloads, signing or DMG mounting.
The original source-equivalence and platform-scope limitations still apply.
CUT-004 is eligible for closure after independent review and normal merge with
the **observation period explicitly waived**, not recorded as observed.

CUT-005 is authorized but **not implemented or accepted** by this documentation
change. No Go source is removed,
no production data is touched and no new release is created here. The preserved
[source handoff](../../handoffs/2026-10-12-rust-continuation.md) records the earlier
paused state; this dated decision supersedes only its observation wait and
separate-authorization blockers. Its source/provenance and privacy limits remain.

The task graph's historical gate identifier `stable-release-plus-7-days` is kept
for traceability. Its observation-duration requirement is waived by this decision;
its other evidence requirements remain mandatory. No old capture is relabeled as
a new passing run.
