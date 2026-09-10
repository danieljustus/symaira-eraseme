# Consent filesystem oracle (ID-005)

`id005.json` is a privacy-reviewed derived capture of the real Go consent
implementation at `93721a93ec9e527410c4ec3051779cf516216b72`. The generator
checks that `consent.go` and `gate.go` are byte-identical to the corrected
contract revision `bf53346eec234929bedf0314b99e3da85dbb991b`. It archives
the complete base, injects only a test helper, and compiles the production
identity package with Go 1.26.6. No Go production files are modified.

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

The Rust consumer is
`identity::consent::filesystem_tests::id005_matches_frozen_go_filesystem`.
It launches each case in a child with its own umask. Comparison is exact for
relative paths, entry types, permission bits, JSON file bytes, old open-handle
bytes, and success/failure. ID-005 is a side-effect contract; native syscall
error text is retained in the raw capture, not normalized into a byte-parity
claim. Existing ID-004 error diagnostics have separate regression tests.

The capture covers fresh/nested creation, read-only existing directory
hardening, replacement of a read-only token, verification/list hardening,
wrong-command preservation, expiry cleanup, umasks 000/077/777, blocked mkdir,
failed temporary creation, and failed rename over a nonempty directory.
Replacement is also read through a handle opened before the update. A stale
`.consent-sentinel.tmp` and a destination sentinel must remain byte-identical;
the complete manifest detects temporary-file leaks.

Go uses `CreateTemp → Write → Close → Chmod → Rename` with deferred temporary
cleanup. It performs **no fsync**; sync failure and crash durability are not
guarantees of this consent writer. Existing directory/validated-file chmod
is best effort; temporary-file chmod is required. Exact random temporary-name
collision and close/chmod fault injection have no existing Go test hook.
Rust's safe file drop cannot report close errors; that limitation remains
open rather than being represented as a passing fault-injection case.

The retained capture was executed on macOS arm64. Unix permission tests must
run as a non-root user; root would invalidate the permission-denial case.
Windows uses Go's writable/read-only mapping rather than POSIX modes, and
the Rust implementation states that mapping explicitly. This fixture does
not establish Windows ACL, replacement, or runtime parity. Native Linux and
Windows, independent review, the remaining matrix, release, and cutover
remain open. The contract matrix status is intentionally unchanged.
