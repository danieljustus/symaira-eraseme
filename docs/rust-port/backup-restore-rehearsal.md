# Historical Go fallback backup and restore rehearsal

This executable rehearsal covers one explicit rollback boundary for RUST-016.
It starts from the checked-in synthetic schema-v1 fixture, reads its three
requests with the supplied retained Go artifact, takes a SQLite online backup,
performs one real Rust campaign write, and records the Go artifact's actual
failure against the resulting schema-v2 database. It then restores the v1
backup into a separate disposable data root and proves that Go again reads the
same three baseline requests while the Rust-created request is absent.

## Native Linux/Windows candidate — 2026-10-03 (#1122)

`backup-restore-native.yml` selects all six native OS/architecture targets and
checks the Rust compiler host before building and executing the candidate.
Its retained Go input is the official v0.12.1 native archive, checked against
both release asset digest/size and `checksums.txt`; only the sole regular
executable member is copied. The runtime additionally verifies the unmodified
archive bytes and embedded source revision `240bf67c`/`vcs.modified=false`.
The same eight-case sequence and partial-restore rejection remain mandatory.

Linux amd64 now uses the existing real namespace/Landlock ABI-4 driver, as
arm64 does. The rehearsal runs native file-read/write, TCP/UDP and child-exec
negative controls before any CLI observation. Its audit records private
namespace identities and any virtiofs remounts actually present; absence of
virtiofs on a hosted runner is not a host-share-remount claim. The separate
full-switchback runner retains its historical arm64/virtiofs requirements.

The new Windows driver launches a suspended AppContainer process with no
capabilities, grants only its unique SID access to the fresh evidence root,
forbids child creation, then assigns an owned kill-on-close Job before resume.
Native controls require real outside-root file denial, TCP/UDP access denial,
allowed owned-root writes, blocked child creation, exact nonzero exit 23 and a
250 ms stalled-process deadline. Cleanup checks the actual Job active-process
count, removes the owned SID ACL and deletes only the UUID profile it created.
Raw stdout/stderr and failure records remain available if any assertion fails.
No administrator trust/network policy, existing profile or operator data changes.
The parent creates SQLite backup/restore and deliberately partial fixture states;
the Windows CLI receives no access to the parent Python runtime.

The workflow also executes the real migration-engine, command-surface and
consented live manual-fallback cases. The accepted #809 deviation remains:
automatic browser clicking is deferred; confirmation is a manual task.
CLI-023/024 remain PARTIAL until these native observations are accepted.
Local archive/SQLite controls and syntax/Go vet checks pass, but the new
Windows confinement and full Linux restore are not yet observed.

At candidate `b88e3d0`, native Linux arm64 job `111230070745` and Linux amd64
job `111230070780` in run `37132442133` passed all eight actual restore cases,
with the required native namespace/Landlock controls and source-bound official
release artifact. The first Windows amd64 job `111230070798` stopped at the
committed-harness byte check: its default Git checkout converted Python files
to CRLF, before any AppContainer or migration execution. The measured Python,
documentation and workflow files now require LF through `.gitattributes`;
hash validation remains exact. The original failure is retained and does not
count as Windows confinement or restore evidence.

## Archive verification preparation — 2026-09-29

At clean candidate `cfdc59a0d9266b67a5541de4562a3e7b702d894c`, all five
`test_plain_backup_restore.py` controls pass on native Darwin/arm64 and
Linux/arm64. The ZIP controls use synthetic executable bytes and check exact
member identity, duplicate/nested/nonregular rejection and read-only inspection
without extraction. Existing tar, SQLite WAL backup and incomplete-restore
controls also pass. These checks neither execute a Windows binary nor establish
an official Windows release identity. Windows confinement and native restore
remain open; the rehearsal still fails closed on unsupported hosts.

## Verified run — 2026-09-26

The complete eight-case sequence passed on macOS arm64 at clean candidate
`2ece12f74a3fda37c217ced14e7aceac7d4e2a7f`, using the pinned Rust 1.98.0
toolchain. The Rust release executable was built from that checkout with
`cargo build --locked --release -p symeraseme-cli --bin symeraseme-rust`; its
SHA-256 was
`ebd7221d8f27865308bffa1214c3ba72dcdd7aa0e22c938bdc046f858ae9fc26`.

The historical Go executable was the actual checked-in
`evidence/artifact/symeraseme-rollback-v0.12.1` bytes, SHA-256
`d2cafdd118ad8c81bd29f7d165949f78dc2722d0b5b043368a0db616d4838f22`.
Its embedded metadata reports Go 1.27.1 and module `(devel)` without a VCS
revision. This identifies the tested binary by bytes; it does not establish a
release or source identity. The checked-in v1 fixture SHA-256 was
`595a4840dbe6a52324b40778c53016b0c809e01451ba4fa5f20c3fd3447e0120`.

Observed data boundary: Go read 3 baseline requests from schema v1; Rust wrote
one request and read all 4 from schema v2; the historical Go binary then refused
v2 with `Go port supports up to 1`. Restoring the pre-Rust online backup returned
the isolated store to v1, and the same Go bytes read all 3 baseline requests.
The Rust-created request was absent after restore. The partial-restore negative
control returned only 2 requests and the baseline verifier rejected it. Thus,
restoring the backup loses every write made after that backup; operators must
preserve or reconcile those post-backup writes before using this rollback path.

Raw command records, database snapshots, and the full report were retained under
the ignored candidate-local path
`target/issue-1035-backup-restore-clean/`. The run recorded a clean source
checkout, no tracked or untracked source changes, and distinct SHA-256 identities
for the Go and Rust executables.

The runner records the supplied Go binary's hash and raw `go version -m` output.
It does not infer release identity from a filename. The repository's retained
`evidence/artifact/symeraseme-rollback-v0.12.1` file reports module `(devel)` and
no source revision, so a run using it records only historical byte identity.
For release-bound evidence, pass the unmodified binary extracted from the
checksum-verified official archive with `--go-archive`; the runner reads the
archive without extracting and requires exactly one regular top-level
`symeraseme` tar member or `symeraseme.exe` ZIP member whose bytes match `--go`.
The declared and streamed member sizes must match the supplied executable before
the SHA-256 comparison; duplicate and nonregular members are rejected. ZIP
verification does not enable the Windows rehearsal: its process/filesystem/network
confinement and native restore acceptance remain open. It records the archive hash and
the binary's embedded source metadata. Any observed schema-v2 failure is
specific to the exact supplied artifact and this rehearsal.

## Release-bound replay — 2026-09-27

The same eight-case sequence passed on macOS arm64 at candidate
`62e18d2efdb098854124f2d859361c4c5d07530f` with the official
`v0.12.1` `symeraseme_0.12.1_darwin_arm64.tar.gz` archive. Its SHA-256
`7fa696829c9bf861ba902a65576d22013e4eeeb655150143975f961078dc906b`
matched the release `checksums.txt` and GitHub asset digest. The exact archive
member tested had SHA-256
`b90ff3e0c16a5bfb6a9c751d79845f74983217b3f0af9d9f74faa3a255e325a3`;
its embedded Go build metadata identifies module version `v0.12.1`, source
revision `240bf67cefa05e643e32611a02e6e7ed87a033ea`, and
`vcs.modified=false`. The current Rust debug binary SHA-256 was
`fa8f3be968c093abb3e00bb0b1fd8e091dd6a075d4da7ecb7fbe632ea027feeb`.

The isolated report is under ignored
`target/issue-1035-release-proof-20260927/run/`. It records all eight cases
passing, schema versions 1 → 2 → 1, a successful restore, and rejection of an
incomplete restore. The official Go binary refused schema v2; the restored
schema-v1 store contained the original three requests and excluded the Rust
write. This proves the backup path for those exact artifacts and fixture.
Native Windows and production-data restore remain unverified.

Run on macOS, or on the native Linux aarch64 sandbox supported by
`plain_store_switchback.py`:

```sh
python3 rust-tests/parity/backup_restore_rehearsal.py \
  --go /absolute/path/to/released-symeraseme \
  --go-archive /absolute/path/to/symeraseme_0.12.1_darwin_arm64.tar.gz \
  --rust /absolute/path/to/worktree-target/debug/symeraseme-rust \
  --go-tool "$(command -v go)" \
  --output-dir /absolute/path/to/new-disposable-evidence
python3 -m unittest discover -s rust-tests/parity \
  -p test_plain_backup_restore.py -v
```

The output directory must not exist. The runner isolates HOME, XDG, temp and
database roots; denies network and unrelated process execution; and retains the
committed harness revision, worktree dirty status, runner SHA-256, caller-supplied
binary hashes, raw command arguments/exit/stdout/stderr, fixture and archive
hashes, schema snapshots, backup and restore snapshots, and an overall report.
The Rust executable is caller-supplied; its build source binding is not inferred
from the separately recorded source checkout. The negative control deletes one
baseline request and its dependent rows from a separate restored copy, observes
the retained Go reader return two requests, and proves the baseline verifier
rejects that incomplete restore.

All writes occur under the new disposable evidence directory. The checked-in
fixture and historical artifact are only read. No operator data, publication,
cutover, Go removal, or cleanup is involved. This macOS rehearsal does not
replace native Windows evidence or make the historical artifact releasable; it
does not close the other consumer-owned prerequisites in issue #1035.
