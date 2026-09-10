# Consent filesystem oracle (ID-005)

`id005.json` is a privacy-reviewed derived capture of the real Go consent
implementation at `93721a93ec9e527410c4ec3051779cf516216b72`. The generator
checks that `consent.go` and `gate.go` are byte-identical to the corrected
contract revision `bf53346eec234929bedf0314b99e3da85dbb991b`. It archives
the complete base, injects only a test helper, and compiles the production
identity package with Go 1.26.6. Helpers are stored as `.go.in` templates so
normal Go package discovery excludes them. No Go production files are modified.

The fixed clock is 1000 seconds and the injected standard-library random
reader returns byte 7. All token bytes are synthetic test inputs; there is
no user data, credential, host path, or native error text in the derived
fixture. Full raw observations, errors, command exit statuses, toolchain,
source/archive/helper/generator/binary digests remain outside the checkout.

Regenerate deliberately, then compare the newly derived file byte-for-byte:

```sh
python3 scripts/consent-oracle/generate.py --evidence /tmp/id005-new-capture
cmp /tmp/id005-new-capture/derived.json tests/fixtures/consent-contract/id005.json
```

The source manifest also binds `profile.go`, which defines the permission
constants. The retained provenance identifies the native capture platform.

The Rust consumer is
`identity::consent::filesystem_tests::id005_matches_frozen_go_filesystem`.
It launches each case in a child with its own umask. Comparison is exact for
relative paths, entry types, permission bits, JSON file bytes, old open-handle
bytes, success/failure, and typed error classifications. ID-005 is a
side-effect contract; native syscall
error text is retained in the raw capture, not normalized into a byte-parity
claim. EEXIST/EISDIR/ENOTEMPTY share the explicit `destination_conflict`
category; all other observed errors retain their distinct category. Existing
ID-004 error diagnostics have separate regression tests.

The capture covers fresh/nested creation, read-only existing directory
hardening, replacement of a read-only token, verification/list hardening,
wrong-command preservation, expiry cleanup, umasks 000/077/777, blocked mkdir,
failed temporary creation, failed rename over a nonempty directory, and a
real partial-write failure with a one-byte soft `RLIMIT_FSIZE` and ignored
SIGXFSZ. Python 3 launches the equivalent bounded-file-size Rust child without
unsafe code; it is a test-only dependency. The file-size limit is confined to
the child and cannot affect the runner or other tests.
Replacement is also read through a handle opened before the update. A stale
`.consent-sentinel.tmp` and a destination sentinel must remain byte-identical;
the complete manifest detects temporary-file leaks.

Go uses `CreateTemp → Write → Close → Chmod → Rename` with deferred temporary
cleanup. It performs **no fsync**; sync failure and crash durability are not
guarantees of this consent writer. Existing directory/validated-file chmod
is best effort; temporary-file chmod is required.

## Source-bound failure probes

`id005-faults.json` is a separate generated capture, preserving the ordinary
15-case fixture unchanged. Run:

```sh
python3 scripts/consent-oracle/generate.py --fault-probes --evidence /tmp/id005-new-faults
cmp /tmp/id005-new-faults/derived.json tests/fixtures/consent-contract/id005-faults.json
```

This mode inserts two hooks **only in the disposable archived Go source**,
immediately before the original checked Close and Chmod operations. Both
anchors must occur exactly once; missing-anchor rejection is exercised before
compilation. The original error checks remain intact. The fixture records
original/instrumented source hashes, exact insertions, helper and generator
hashes. Raw evidence additionally retains the source diff, binary digest,
native platform, verbatim errors and one executed Go test per case.

- `close_failure`: the hook successfully closes the temporary `os.File`.
  The original checked Close then returns real `os.ErrClosed`; deferred cleanup
  removes the temporary file. This proves the Go error branch and rollback,
  **not** a delayed native EIO/ENOSPC close failure.
- `chmod_failure`: the hook successfully unlinks the temporary path.
  The original Chmod returns real ENOENT. Both Go and Rust preserve the old
  0400 token, its open-handle bytes, and the unrelated 0600 stale sentinel.
  Because the injector removed the temp, this does not prove cleanup following
  a chmod error while the temporary file still exists.

The Rust consumers are the three `id005_atomic_*` tests in
`consent_filesystem_tests.rs`. The chmod case calls the actual required chmod
operation after unlink, and compares the observed error category and complete
filesystem manifest with Go. The close test performs real checked close, then
injects a typed adapter error; it checks exact error propagation and compares
only failure/rollback effects with Go. It explicitly does **not** compare that
injected error with Go's `os.ErrClosed` or claim native close-fault parity.
A corrupted-sentinel negative control exercises the actual comparator.
`id005_fault_fixture_is_bound_to_source_and_probe` rejects source/helper/generator
drift.

## Checked close and retained sync contract

On Unix, Rust now splits the temporary file from its `TempPath` guard and
calls the pinned `nix 0.31.3` safe `close(File)` API before chmod/rename. That API
consumes ownership, invokes close once, and returns errors without retrying
or treating EINTR as success. The path guard remains alive across checked
close and chmod failures. No raw descriptors or unsafe code are introduced
in this repository. The dependency implementation was inspected in the local
pinned source (`nix/src/unistd.rs`); see the
[upstream API and source](https://docs.rs/nix/0.31.3/nix/unistd/fn.close.html).
The alternative `io-close 0.3.7` was rejected because it normalizes EINTR to
success. Non-Unix retains the existing drop behavior and remains blocked on
a reviewed checked-close adapter and native execution evidence.

`sync_all` remains mandatory before checked close. Its error is returned
unchanged, publication stops, and the owned temporary file is cleaned up.
The Rust-only sync test injects a typed error and checks the full unchanged
manifest, old open-handle bytes, and that later stages are unreachable. It
is an adapter/control-flow test, not a native fsync fault. Go has no equivalent
operation, so the fixture's `blockers.sync` explicitly records this difference;
there is no fabricated Go sync-error row. This checkpoint retains the existing
safety guard pending coordinator contract review. Removing sync or declaring
its extra failure behavior acceptable parity requires a separate reviewed
contract decision; checked close alone does not settle durability semantics.

Delayed native close EIO/ENOSPC needs a controlled faulting filesystem or an
approved native syscall injector, neither available in this probe. Exact
chmod permission failure with an extant temp needs a deterministic native
fault mechanism; unlink cannot substitute for that evidence. Exact random
temporary-name collision is also still unproven. These limitations, native
platform gaps and coordinator acceptance keep ID-005 incomplete.

The retained capture was executed on macOS arm64. Unix permission tests must
run as a non-root user; root would invalidate the permission-denial case.
Windows uses Go's writable/read-only mapping rather than POSIX modes, and
the Rust implementation states that mapping explicitly. This fixture does
not establish Windows ACL, replacement, or runtime parity. Native Linux and
Windows, independent review, the remaining matrix, release, and cutover
remain open. The contract matrix status is intentionally unchanged.
