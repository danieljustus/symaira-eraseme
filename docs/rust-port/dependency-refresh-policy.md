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
Windows dependency-resolution changes. Genuine captures are retained from
Rust CI run `37796278728` and HTTP auxiliary run `37796706488`, both bound to
`411b5c10eb041f0233e0e86710a3dd733b0012e2`. Refreshed candidate native acceptance
remains pending; this document alone is not evidence of executed native tests.

## Publication privacy derivation

The retained native grant captures contain randomly issued local consent
identifiers. All six captures revoke their isolated grants and record an empty
final grant store, but archived issuance and consent-record copies still contain
those identifiers. Publish privacy-derived genuine captures, not raw copies.

`scripts/derive_frozen_grant_privacy.py` consumes the independently reviewed
selection and requires its explicit SHA-256. It replaces only the two random
grant identifiers per native target with same-length public test identifiers,
consistently updating revoke arguments, issuance/list outputs, consent-record
tokens, token-derived filenames and dependent frame digests. Recursive JSON
checks preserve keys, types, case counts, commands, times, modes, source/build
identities and all other behavior-relevant values. Non-grant bytes are unchanged.
It rejects altered input digests and emits original/derived hashes and changed
JSON paths in `privacy-derivation.json`; original capture ZIPs, selected raw bytes
and historical repository fixtures remain unchanged. This publication step does
not approve native replay or relax the strict Cargo-lock/source-binding rule.

The initial derivation starts from capture commit
`411b5c10eb041f0233e0e86710a3dd733b0012e2` and reviewed selection digest
`e5d75af3feeebcc7cff13dbbdf65493bc0c03c38147119412823f0e63314ea31`.
The 202 selected files include 54 privacy-changed grant files and 148 unchanged
files. Independent publication review approved these exact derived bytes and
the twelve reader proposals before integration. The committed sidecar is
`tests/fixtures/go-frozen/refresh-411b5c10/privacy-derivation.json`, SHA-256
`b863b850df548f745ecd68967c52fc0fb4f74cfb4cd53ed59f09fe74ee9b94c0`.
It records each artifact ID, ZIP digest, original member, native target and
original/derived file digest. Raw ZIPs remain retained outside the repository.
Publication privacy approval does not clear the unchanged frozen/native
acceptance gates, which must execute on the integrated candidate.
