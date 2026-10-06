# Plain-store executable switchback regression

`rust-tests/parity/plain_store_switchback.py` exercises one bounded macOS
plain-store sequence through the real CLI. It is not release acceptance and
does not close #1035 or CUT-003.

```sh
python3 rust-tests/parity/plain_store_switchback.py \
  --go /absolute/path/to/source-built-go \
  --rust /absolute/path/to/source-built-rust \
  --go-tool /absolute/path/to/go \
  --output-dir /absolute/path/to/new-evidence-directory
python3 -m unittest discover -s rust-tests/parity -p test_plain_store_switchback.py -v
```

The output directory must not exist. Failures, raw stdout/stderr, command exits,
artifact identities, policy controls and database snapshots are retained there.
The source fixture and historical `evidence/rollback.json` are never overwritten.

The required case sequence is Go baseline, Rust write, Rust plan/readback, Rust
request readback, actual Go executable switchback, then Go plan/request readback.
Current Go migrates the checked-in synthetic fixture first: the schema sequence
is **1 -> 2 -> 2 -> 2**, not a Rust-caused upgrade. The three baseline requests
must survive Rust's newly persisted campaign/request. Complete semantic JSON and
all SQLite table values, column definitions, schema/index objects, user_version
and integrity verdict must match after switchback. No database restore occurs.
JSON object key order is non-semantic; arrays, numeric types, null/missing fields,
large integers and stored BLOBs remain distinct. Raw page-image equality is not
claimed.

Both artifacts must differ. The actual Go build metadata must report Go 1.26.6,
CGO disabled, and the native Darwin architecture. Artifact hashes alone do not
prove source identity: retain independent build records. The `Native store switchback`
workflow reuses the prerelease workflow to build and verify the six
release-candidate archives, extracts each target's Rust `symeraseme` and
packaged `symeraseme-go` after checking `checksums.txt`, runs the plain and
encrypted runners on Linux, macOS and Windows (amd64 and arm64), and uploads
raw observations even on failure. Linux creates the host-share write probe only
when a writable virtiofs share exists (VM hosts); hosted runners have none. The local exploratory precedent is the independently
reviewed capture at source `8986a3db3d60d37b89368d37e15f1e98c60672f2`, indexed by
`ceae1721336d6a682345d51f1dd4bcd966a53845e3005c8f4a3ebf38e1582da1`;
that capture is separate from this new repository gate.

Runtime uses empty PATH, isolated HOME/USERPROFILE/XDG/temp roots, an absent
profile and explicit unencrypted storage. macOS sandboxing denies network,
other executable launches and operator-home/repository contents outside the owned
run directory. Only metadata of the run directory's exact ancestors is readable:
SQLite needs it when the run is nested under HOME or the checkout (as in CI).
Sibling file contents and HOME directory enumeration remain denied. A regression
checks SQLite access plus these denials with outside/HOME/checkout run layouts.
Harmless denial probes run before the real CLI. Commands have a
30-second deadline, process-group cleanup and a 4 MiB per-file write ceiling.

Unsupported hosts fail explicitly. Native Linux/Windows confinement and cleanup,
encrypted storage, crash/concurrent-writer recovery, installed or released
artifacts, publication and production cutover remain separate unproved gates.

## Published Go archive preparation — 2026-10-04

`scripts/fetch-retained-go-release.py --output-dir /absolute/private/directory`
fetches the pinned actual published `v0.12.1` archive for the current native
target. It checks the measured release checksum file, archive and executable
SHA-256, exact sizes, and native clean Go 1.26.6 build metadata without running
a Go SDK. The six archive identities are pinned in
`tests/fixtures/go-rollback/v0.12.1.json`. The bounded Python metadata reader's
output independently matches actual Go SDK metadata for all six binaries.
With Go actually absent from PATH, all six archive consumers pass and each
rejects a symlink, foreign target, same-size corrupted binary and corrupted
archive. These are archive/metadata checks; foreign binaries are not executed.

The optional `--published-go-release` replaces `--go-tool` for metadata
verification only. It preserves every existing host, confinement, real-process,
six-case and complete-database assertion. Python metadata is recorded as
`go-artifact.metadata.txt`, never as fabricated Go process stdout.

The actual Linux amd64 release binary
`31fc9c4dd33c76ed0b9fc443079af5d3542e7d5a3f31b76d4d8a2f75738d4099`
reads the owned schema-v1 fixture successfully, but exits 1 against schema v2
with `Go port supports up to 1`. Its clean source revision is
`240bf67cefa05e643e32611a02e6e7ed87a033ea`. This private native compatibility
probe is separate from confined switchback acceptance and agrees with the
historical release-bound backup rehearsal. Consequently, this published
archive cannot satisfy the preservation switchback gate. The workflow continues
to build both implementations from the candidate; its result does not establish
published-release compatibility. A compatible published fallback and native
acceptance remain required before promoting the rollback/cutover rows. The
archive preparation leaves issue #1131 open.

The original `backend_fallback_process` test can also select this exact
published rollback sibling with `SYMERASEME_ROLLBACK_GO_BINARY` set to the
fetcher's binary path. It verifies the pinned whole manifest and native
executable hash before staging the real sibling. Its original four CLI
status/stdout/stderr comparisons, native/fallback MCP initialize comparison,
missing-sibling failure and invalid-backend rejection remain unchanged.
`SYMERASEME_PARITY_LIVE_GO=1` still builds and compares actual current Go.
The Go SDK is absent in the default dedicated job; the published runtime stays
outside PATH and is used only for this explicit rollback test until removal.
The six-native verification script requires the same original case to execute
with zero skips. Native acceptance of this new mode on five other hosts is
pending; this fallback test does not exercise database upgrade or preservation.

At clean `486165456bf328a45f09a6529e507953f92f7bfd`, the exact dedicated
SDK-free workflow passes **376 tests, zero failures** on Linux amd64. The
delegated consent parent executes all sixteen private cases. All twenty frozen
native records and the published-release manifest remain byte-identical to Git
blobs and an actual autocrlf checkout. Three actual compiled rejection runs
prove the published sibling guard rejects a same-size changed binary, symlink
and foreign native target before comparison.
