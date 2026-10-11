# Published Rust-only evidence reconciliation (2026-10-11)

Status: retained release evidence verified; prior R1/R2 corrections passed
independent delta review. This publication-safe documentation candidate and its
full intended push history await fresh independent SPEC/evidence and
QUALITY/security review. The migration is not complete. CUT-004 observation and
CUT-005 separately authorized retirement remain open. REL-006's literal
read-only mounted-manifest gate is PASS only for the exact existing public
`v0.14.1` DMG identified below.

## Identities and retained evidence

- Main/code candidate: `89a0fbcdef745714950ca1994ceb11c0e8b832ab`.
- Stable immutable release: [v0.14.1](https://github.com/danieljustus/symaira-eraseme/releases/tag/v0.14.1), release ID `409043734`, commit
  `587e9e0e3aaa4d4bbacdd28a7087d17ac2748548`; annotated tag object
  `0e5bec0accfc7b8b9617d55f27f52da1315f8cdd`.
- All 21 original asset IDs, names, sizes and digests still match the retained
  stable-promotion receipt. Every asset was freshly downloaded through its
  public HTTPS URL and verified against the release API and checksum manifests.
- Original native artifact ZIPs, raw job logs, public assets and executable
  readback scripts are retained privately. The private `package-handoff.json`
  indexes the receipts; `final-execution.json` records successful execution of
  `verify.py`, `verify-native.py` and `summarize.py`. Receipt and script names
  below identify privately retained evidence, not repository files or public
  downloads; only explicit HTTPS links point to public evidence locations.

The release run [38063683853](https://github.com/danieljustus/symaira-eraseme/actions/runs/38063683853)
is **not** represented as wholly successful: its final publication step failed.
The six binary jobs, archive/SBOM assembler, six store jobs, CLI staging and GUI
job succeeded. Independent public readback and all eight strict attestations
verify the subsequently published original assets. No release was rerun here.

The privately retained REL-006 parent verification (`parent-verification.json`)
binds the public DMG [Symaira-EraseMe-0.14.1-macos.dmg](https://github.com/danieljustus/symaira-eraseme/releases/download/v0.14.1/Symaira-EraseMe-0.14.1-macos.dmg), asset `628363023`,
size `8694828`, SHA-256
`ab90f1fd9c6d123def8334129e8ee3472fe32e3f88129ea77c20e1d29bb39859`,
to actual read-only `hdiutil` execution on the authorized Mac Mini,
macOS 15.8.1/x86_64. In that privately retained evidence package,
`release-readback.json` binds the public release/asset identity; `probe.py`
is the executed source; `capture/report.json` binds ten passing checks to
`capture/mounted-manifest.json` (25 unique entries) and the downloaded image.
The report SHA-256 is
`43ed5b16351c02e6904087a4edac96df4727e641303710cc4afcc4e039a42ce3`.
Raw `capture/attach.stdout` records the mounted device, and
`capture/detach.stdout` confirms normal detach. Download, attach and detach
exited 0 without timeouts; the report records mount absence after cleanup,
and the parent independently confirmed absence by live SSH readback.
All ten mounted app regular-file hashes match the retained HFS extraction.
This correction revalidates those retained bytes; it performs no new mount,
app execution, signing, publication or policy change.

Earlier local failures remain unchanged in `dmg-mount-receipt.json`,
`dmg-mount.stderr` and `diskutil-mount-receipt.json`: permission denial and a
45-second timeout, respectively. The original `review-9da7ce94/review.json`
also remains unchanged. Its missing-mount conclusion is superseded by the
parent proof above, not by an HFS-extraction waiver; its R1 alternate-waiver
and R2 input-scope findings remain valid for the old documentation candidate
`9da7ce944aab6073bc0ab2020c6b5298594aad19`. Their corrections passed independent
SPEC/evidence and QUALITY/security delta review at
`8f2111b2f942f1495bac7dcdc112a0b801c2ac02`. That retained review is unchanged;
it does not approve this new publication candidate or its intended push history.
The subsequent publication check found machine-local evidence references
(PUB-001). This candidate removes them without publishing the prior local
commits; fresh independent review of the final documents and history is pending.

## Current acceptance, not historical canary status

| Row | Current evidence and verdict |
| --- | --- |
| REL-001 | PASS. Exactly six expected `symeraseme_0.14.1` CLI archives, built in the original six native release jobs. |
| REL-002 | PASS. Every complete archive root contains only the regular executable, `LICENSE`, and `README.md`; executable tar modes and duplicate/link rejection are checked by the existing archive verifier. No Go sibling. |
| REL-003 | PASS. Seven archive/DMG checksum entries and twelve SBOM-sidecar entries match all public bytes and API digests. The six CLI archives are byte-identical to same-run staging artifact `11674630384`. |
| REL-004 | PASS. Original native build/smoke/linkage steps succeeded on all six targets. Fresh macOS arm64 checks execute the public archive CLI and signed app helper with network, writes and operator-home reads denied; both return the exact 0.14.1 JSON version. |
| REL-005 | PASS. Homebrew run `38085163720` on current main passed all four native install/test jobs and its protected-PR publisher. The current public Formula exactly equals the tested rendered bytes and matches all four public URLs/checksums; tap commit `872cbd7ffe5de1e72f14d2cc2537fab369a8068d` merged PR #42. |
| REL-006 | PASS for the exact existing public DMG identified above. The literal read-only OS-mounted manifest contains exactly the two regular executables `Symaira EraseMe` and `symeraseme` in `Contents/MacOS`, bundle ID `com.symaira.eraseme` and version `0.14.1`. All ten native checks pass; 25 unique manifest entries and successful detach/cleanup are retained. Earlier local mount failures and HFS extraction remain historical/supporting evidence, not an alternative acceptance route or whole-migration acceptance. |
| REL-007 | PASS. The actual extracted nested Rust helper satisfies the Developer ID team requirement `M4744F3TAA`; strict app/deep verification also passes. |
| REL-008 | PASS. The downloaded original DMG passes strict container-signature verification. |
| REL-009 | PASS. App and DMG both pass fresh `stapler validate` and Gatekeeper checks; published release notes describe Rust-only distribution. HFS extraction is not relabeled as OS mounting or a fresh GUI-window test. |
| REL-010 | PASS within the published-artifact scope. Same-run job `114248617073` reproduced the packed Rust inventories and twelve sidecar hashes; all resulting public sidecar bytes are unchanged. Eight fresh `gh attestation verify` calls bind six archives, the SBOM manifest and DMG to the exact release source, tag and `release.yml`. Source-equivalent audit/deny and coverage evidence is identified below. |
| CUT-001 | PASS. Named retired-selector regression passed on all six current native jobs and all six original release-binary jobs. Public roots have no Go executable. |
| CUT-002 | PASS. Twelve original native plain/encrypted reports consume the exact checksum-verified binaries subsequently published in all six archives. Existing copied data and encrypted MCP reads pass. |
| CUT-003 | PASS within the development-oracle switchback contract. The same original runs replace Rust with the separately built Go oracle and read back the Rust-written data. All 72 declared/executed case IDs, typed success/exit results and 144 stdout/stderr streams revalidate. Plain schema sequence is 1,2,2,2; complete SQLite state agrees, and the encrypted V3 store retains four requests. No backup restore occurred. This does not prove an older published Go binary can read schema v2. |
| CUT-004 | OPEN. Stable publication is verified; seven days and genuine defect-free observation evidence are not yet available. |
| CUT-005 | OPEN. Go remains the development oracle. No source/toolchain retirement is authorized or performed by this package. |

Store evidence comes from six independently digest-checked original Actions
ZIPs, bound to run `38063683853` and release source `587e9e0e`. This is retained
native revalidation, not a fresh six-target runtime run. Eight local rejection
controls reject altered case inventory, false success, changed streams and
wrong binary identity. Synthetic controls are not native observations.

## #1131 and exact native case receipts

Current-main Rust CI [38083487544](https://github.com/danieljustus/symaira-eraseme/actions/runs/38083487544)
passes all six native jobs. The original logs and API step conclusions jointly
verify format, all-target/all-feature check and strict Clippy, workspace tests,
doctests, and the scoped Go-absent wrapper. The default workspace selects frozen
mode (`SYMERASEME_PARITY_LIVE_GO=0`); this is not a claim that every workspace
case ran with the Go SDK removed.

| Native job | Workspace/doctest passing cases | Ignored entries | Named Go-absent wrapper cases |
| --- | ---: | ---: | ---: |
| Windows arm64 `114305036499` | 619 | 3 | 3 |
| Windows amd64 `114305036574` | 619 | 3 | 3 |
| macOS amd64 `114305036525` | 674 | 6 | 6 |
| macOS arm64 `114305036622` | 674 | 6 | 6 |
| Linux amd64 `114305036532` | 675 | 6 | 6 |
| Linux arm64 `114305036565` | 675 | 6 | 6 |

Every scoped expected name is matched to an actual `... ok` line, including
`retired_backend_override_never_dispatches_a_go_sibling`, both whole-plan cases,
`native_bind_failures_match_checked_out_go`, the applicable HTTP process/header
cases and corrupt/truncated/fabricated-record controls. Windows additionally
uses the fully qualified `windows_acl::native_windows_token_dacl_and_rotation_match_go`.
Ignored and platform-inapplicable entries are retained explicitly, not counted
as passes. `native-reader-verification.json` contains the full observed inventory
and summaries, including ignored names and source-derived required selections.

The Linux-only dedicated Go-absent run `38059031343`, job `114233315532`, passes
430 tests with two parent-launched helper entries ignored. Its source is
`5850fa0d3a08c2f849748713d3c9fe5238798c25`, not current main. The retained
`verify-native.py:52-57` and `native-reader-verification.json` record a scoped
tracked-source/configuration comparison against code/main `89a0fbcd`: the
explicit path inventory is `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
`.cargo`, `crates`, `rust-tests`, `tests/fixtures`, `cmd`, `internal`, `go.mod`,
`go.sum`, `Makefile`, `.github/workflows/rust-no-go.yml` and
`scripts/verify-frozen-plan-without-go.py`. These tracked paths are unchanged
across that source, release and code/main. `source-equivalence.json` retains
the full tracked release/main inventories and their six-file diff. The consumed
`registry/` bytes are outside the selected path list; their unchanged state is
checked separately across the same revisions and recorded in
`review-9da7ce94/support-results.json` (`registry_tracked_bytes_unchanged`).
This is retained source-equivalent Linux execution evidence, not a complete
compiler-input closure, external toolchain/environment identity, fresh build
or new native run. It does not turn `rust-no-go.yml` into a six-platform
Go-absent workflow. No duplicate workflow was dispatched.

The 28 original HTTP records selected by digest
`8a6d3eb4853adb7cfe176991b50b3cb3f04185e1e47b00478ab0f0c35f242db4`
match eight freshly retrieved original ZIPs from `37834833493` / `37834833576`,
producer `5df99f8f5178ba9ae07e6c12be5067ae121e115b`. Every recorded source and
generator size/hash matches that immutable source. Current differences are
limited to four local Cargo package version labels and a noncompiled
`release_pipeline_test.go` step-name update, as explicitly allowed by the
existing `go_source_pin.rs` policy. Dependencies were not normalized away.
All four Unix targets have real source-bound records and current reader passes.
The Oct 8 note's then-pending macOS arm64 producer job subsequently completed;
the committed reviewed selection identifies its actual Actions artifact. The
historical selection's `frozen_reader_acceptance: pending` remains unchanged;
this receipt records later reader acceptance without rewriting original bytes.

Current native target proof `38083487548` separately passes the SQLite portability
case and release-binary version smoke on all six targets. It is not substituted
for HTTP/process/native frozen-reader acceptance.

## Controls, security evidence and source equivalence

The release-to-main diff contains six files: two publisher workflows, the frozen
grant privacy derivation script and three Python control files. Product source,
Cargo manifests/locks/toolchain, app inputs and native store/reader harnesses are
unchanged. `source-equivalence.json` retains both complete tracked Git inventories
and the actual diff; changed publisher controls are not treated as unchanged.

Rust PR fast gate `38059031306`, job `114233316059`, on `5850fa0d` passed nextest,
doctests, strict Clippy, dependency audit/deny and coverage. Reuse at release/main
is supported by the scoped tracked-source/configuration comparison above, not
complete compiler/environment closure. The recorded critical coverage is 6465/6978
lines (92.65%, required 90%). This is retained source-equivalent evidence, not a
new coverage measurement. Actual same-source hardening run `38061587742` has
successful Miri/fuzz jobs; its skipped mutation jobs are not mutation acceptance.

Fresh noncompiling Python controls execute 21 tests normally and the same 21
under `python3 -O`, with no skips: immutable publication, Homebrew publishing,
release wiring, archive validation/staging and frozen-grant privacy guards.
These are control tests, not substitutes for public publication evidence.
No compiler, paid provider, operator store, mail or removal request was used.

## Observation and resumption boundary

The original stable-promotion receipt and
[the original #1132 observation comment](https://github.com/danieljustus/symaira-eraseme/issues/1132#issuecomment-6100593604)
corroborate a conservative observation start of
`2026-10-10T17:38:53.187732+00:00`. Earliest seven-day review is
`2026-10-17T17:38:53.187732+00:00` (19:38:53 CEST). Elapsed time alone does not
clear CUT-004. No daily observations have been fabricated.

Next: obtain fresh independent SPEC/evidence and QUALITY/security review of
these final documents and the entire intended push history, including the
PUB-001 publication-boundary repair and preserved R1/R2 corrections. The retained
literal read-only mounted-DMG proof needs no remount, HFS-alternative acceptance,
native matrix rerun or new release.
Retain real observation evidence until the eligible review; only then consider
separately authorized CUT-005. Protected AGENTS metadata follow-up #1214 remains
outside this package. The coordinator owns any push, PR or merge.
