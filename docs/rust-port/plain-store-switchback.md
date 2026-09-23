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
prove source identity: retain independent build records. The dedicated
`Plain-store switchback` workflow builds both binaries from its checked-out
candidate, records the revision/clean state, runs all six cases and uploads raw
observations even on failure. The local exploratory precedent is the independently
reviewed capture at source `8986a3db3d60d37b89368d37e15f1e98c60672f2`, indexed by
`ceae1721336d6a682345d51f1dd4bcd966a53845e3005c8f4a3ebf38e1582da1`;
that capture is separate from this new repository gate.

Runtime uses empty PATH, isolated HOME/USERPROFILE/XDG/temp roots, an absent
profile and explicit unencrypted storage. macOS sandboxing denies network,
other executable launches and operator-home/repository reads outside the owned
run directory. Harmless denial probes run before the real CLI. Commands have a
30-second deadline, process-group cleanup and a 4 MiB per-file write ceiling.

Unsupported hosts fail explicitly. Native Linux/Windows confinement and cleanup,
encrypted storage, crash/concurrent-writer recovery, installed or released
artifacts, publication and production cutover remain separate unproved gates.
