# Frozen-capture dependency refresh policy

Decision for #1188: retain strict current-input dependency binding. Candidate
changes to dependency versions, checksums or generator inputs require genuine
new native captures before frozen-reader acceptance. The existing allowance
for the four local release-version labels is unchanged.

## Refresh procedure

1. Freeze a clean candidate commit including the proposed dependency update.
   Dispatch Rust CI on that exact branch with `capture_frozen_oracles=true`.
   The existing producers capture actual Go 1.26.6 service and process results
   on Linux, macOS and Windows, each on amd64 and arm64.
2. Preserve the raw artifacts and GitHub artifact IDs, ZIP digests, run ID and
   full source revision. Validate every applicable target/family, recorded
   source and generator bytes, native build identity and complete cases before
   selecting the new capture. Historical originals remain retained under
   their original provenance; never relabel an old result as the new commit.
3. Deliberately update the selected fixture and independently reviewed digest
   anchors from verified new bytes. Preserve all existing rejection controls
   and the strict `go_source_pin::assert_current_matches_archive` rule.
4. Verify frozen readers without Go and the full native acceptance matrix on
   the refreshed candidate. A capture/upload may succeed while old readers
   fail; that is preparation, not acceptance. The dependency PR remains
   unmergeable until refreshed readers and exact-head gates pass.

No separation of historical oracle provenance from candidate dependencies,
assertion relaxation, automatic fixture approval, release publication or Go
retirement is authorized by this policy.

## Initial refresh

The first candidate includes the complete lockfile change proposed by
Dependabot PR #1185, including signal-hook 0.4.4 to 0.4.5 and its recorded
Windows dependency-resolution changes. Capture and reader acceptance are
pending; this document is not evidence of executed native tests.
