# Historical Go fallback backup and restore rehearsal

This executable rehearsal covers one explicit rollback boundary for RUST-016.
It starts from the checked-in synthetic schema-v1 fixture, reads its three
requests with the supplied retained Go artifact, takes a SQLite online backup,
performs one real Rust campaign write, and records the Go artifact's actual
failure against the resulting schema-v2 database. It then restores the v1
backup into a separate disposable data root and proves that Go again reads the
same three baseline requests while the Rust-created request is absent.

The runner records the supplied Go binary's hash and raw `go version -m` output.
It does not infer release identity from a filename. The repository's retained
`evidence/artifact/symeraseme-rollback-v0.12.1` file reports module `(devel)` and
no source revision, so a run using it records only historical byte identity.
For release-bound evidence, pass the unmodified binary extracted from the
checksum-verified official archive; preserve that archive checksum alongside
the run. Any observed schema-v2 failure is specific to the exact supplied
artifact and this rehearsal.

Run on macOS, or on the native Linux aarch64 sandbox supported by
`plain_store_switchback.py`:

```sh
python3 rust-tests/parity/backup_restore_rehearsal.py \
  --go /absolute/path/to/verified-retained-go/symeraseme \
  --rust /absolute/path/to/worktree-target/debug/symeraseme-rust \
  --go-tool "$(command -v go)" \
  --output-dir /absolute/path/to/new-disposable-evidence
python3 -m unittest discover -s rust-tests/parity \
  -p test_plain_backup_restore.py -v
```

The output directory must not exist. The runner isolates HOME, XDG, temp and
database roots; denies network and unrelated process execution; and retains a
case inventory, raw command arguments/exit/stdout/stderr, fixture and executable
hashes, schema snapshots, backup and restore snapshots, and an overall report.
The negative control changes one baseline campaign identifier in a separately
restored copy, runs the same Go reader, and proves the exact-baseline validator
rejects that partial restore.

All writes occur under the new disposable evidence directory. The checked-in
fixture and historical artifact are only read. No operator data, publication,
cutover, Go removal, or cleanup is involved. This macOS rehearsal does not
replace native Windows evidence or make the historical artifact releasable; it
does not close the other consumer-owned prerequisites in issue #1035.
