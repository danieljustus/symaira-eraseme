# Historical Go fallback backup and restore rehearsal

This executable rehearsal covers one explicit rollback boundary for RUST-016.
It starts from the checked-in synthetic schema-v1 fixture, reads its three
requests with the supplied retained Go artifact, takes a SQLite online backup,
performs one real Rust campaign write, and records the Go artifact's actual
failure against the resulting schema-v2 database. It then restores the v1
backup into a separate disposable data root and proves that Go again reads the
same three baseline requests while the Rust-created request is absent.

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
`symeraseme` member whose bytes match `--go`. It records the archive hash and
the binary's embedded source metadata. Any observed schema-v2 failure is
specific to the exact supplied artifact and this rehearsal.

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
