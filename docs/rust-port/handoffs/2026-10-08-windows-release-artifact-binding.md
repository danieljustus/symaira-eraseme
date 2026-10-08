# Exact Windows release-artifact binding

## Observed PE byte drift

Issue #1184 compared two successful native builds at source
`4e33d57475f286e2add83324b30c66dbad801111` (merge checkout
`73fb950075fa6df94a4cb9c96e9b6207111d6bbb`, identical Git tree):

- Standalone archive run: [37653688556](https://github.com/danieljustus/symaira-eraseme/actions/runs/37653688556).
- Copied-store archive/test run: [37653689087](https://github.com/danieljustus/symaira-eraseme/actions/runs/37653689087).

The four original native binary artifacts were downloaded and authenticated
against their GitHub artifact ZIP digests on 2026-10-08. Parsing the actual PE
COFF headers, section tables and debug directories found:

| Target | Standalone binary SHA-256 | Copied-store-tested binary SHA-256 |
|---|---|---|
| Windows arm64 | `a4dd419eff1dc19f41f99a16c150341e7b72bb7ce61f4bf8a953f66207f1ac47` | `63bd6d51de4ab87e1f95ab5605fc251bc4e14807f6f2b7385e30d8812f84f280` |
| Windows amd64 | `90334d17a92e70182cf94fab4d03bf5d64e2b186a0b2ac066b989294592be64f` | `89c7caaa1217ecb86f14dc3d76ec732125bc4165a7e6ec948c1ef73eebddf7e4` |

Both pairs have identical file sizes, section layout and every raw section
except `.rdata`. All differing bytes are accounted for by:

- The COFF `TimeDateStamp` at file offset 280.
- `TimeDateStamp` in three debug-directory entries (types 2, 12 and 13).
- The 16-byte GUID in the type-2 CodeView `RSDS` record.

Arm64 has 24 differing bytes. Its timestamps are 1791391454 and 1791391618;
its debug-entry offsets are 10880880, 10880908 and 10880936, and its CodeView
GUID occupies offsets 10881432 through 10881447.
Amd64 has 20 differing bytes. Its timestamps are 1791391562 and 1791391660;
its debug-entry offsets are 12089408, 12089436 and 12089464, and its CodeView
GUID occupies offsets 12090468 through 12090483.

Original artifact IDs are 11497608488 / 11498227874 (arm64) and
11497208965 / 11497458743 (amd64). The timestamps and CodeView GUID explain
where these observed byte differences reside. This is not a claim that every
future rebuild differs only in metadata, that a linker option fixes
reproducibility, or that normalized/semantic hashes authorize publication.

## Publication contract

Release publication must execute the existing native copied-store gate on
its own replacement build, rather than borrow acceptance from an earlier
source-identical shadow build:

1. `release.yml` calls the reusable `plain-store-switchback.yml` gate.
2. That gate calls `rust-prerelease.yml` exactly once to build and verify the
   six Rust-only archives and their artifact-bound SBOMs.
3. All six native copied-store jobs consume those same-run archives, verify
   original archive hashes and record the extracted binary hashes. Plain and
   encrypted copied-store checks must both pass.
4. Only successful completion allows the publisher to download the same-run
   `symeraseme-rust-prerelease-archives` artifact, attest and publish it.
   The publisher neither rebuilds nor imports a different workflow run.

Every new release build therefore gets its own exact-artifact acceptance.
Original hashes remain the trust boundary. Historical `v0.13.0` assets and
frozen Go captures remain unchanged; the development oracle is used only by
compatibility checks and is not distributed.

The local wiring regression does not prove live tag publication, OIDC,
signing/notarization, Homebrew installation or seven-day stable observation.
Those independent release gates remain mandatory.
