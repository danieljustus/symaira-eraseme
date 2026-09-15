# Read-only identity profile prerequisite

Scope: the shared core read API needed by CLI-016. This is not completion of
CLI-016, profile management, or bidirectional ID-001. Go remains the production
route and rollback implementation.

## API

`symeraseme_core::identity` exports:

- `ProfilePaths::from_process()` and `ProfilePaths::new(home, environment)`;
- `profile_exists(path: &Path, paths: &ProfilePaths) -> bool`;
- `load_profile<K: KeyringBackend>(path: &Path, paths: &ProfilePaths,
  keys: &mut MasterKeyResolver<K>) -> Result<Profile, ProfileError>`;
- `Profile`, `ProfileAddress`, and opaque `ProfileError`.

Use an empty path for discovery and the existing
`MasterKeyResolver::from_process()` in a production adapter. Tests inject
`FakeKeyring` and explicit paths instead. No call creates a master key or writes
profiles/directories. The caller owns the returned profile's lifetime; this new
read primitive does not introduce Go's process-global plaintext cache.

The CLI adapter should populate the existing `RenderContext` from the public
profile fields, without implementing another loader. Address state/date fields
remain available in `ProfileAddress`; the existing render address has only
street/city/postal_code/country. No CLI dispatch or template behavior changes
are included in this prerequisite.

## Frozen read behavior

The immutable production oracle is
`bf53346eec234929bedf0314b99e3da85dbb991b`, not the separate release/performance
baseline. `profile_read.py` archives its real identity and transitive eventstore
sources and module files, verifies every archived byte, builds with Go 1.26.6,
and injects a fake keyring before invoking the real `ProfileExists` and
`LoadProfile`. Synthetic TEST profiles and keys are the only runtime inputs.

The committed capture contains 46 cases. Most ciphertext is emitted by the real
`EncryptProfileWithKey`; four authenticated variant-header inputs are sealed
with Go's standard AES-GCM primitive and evaluated by real `LoadProfile`.
Randomized ciphertext is retained, not regenerated in check mode. Go replay
re-evaluates the retained inputs before Rust consumes their expected results.
Generator and validator digests are separate from immutable production hashes.

Covered: full/minimal/null profiles, nil normalization, case-insensitive and
duplicate scalar fields, unknown fields, optional address values, empty or
malformed files, malformed plaintext/types, authentication failure, wrong or
missing key, header-before-key ordering, canonical/legacy precedence, explicit
and environment/home discovery, directories and file-as-parent failures.
Nonzero envelope versions and algorithm labels do not select another cipher:
the unchanged original header bytes are AES-GCM AAD, as in Go.

The actual Go load chain normalizes slices then clones the profile. Clone uses
`append([]string(nil), values...)`, so empty string slices serialize as `null`,
while empty addresses serialize as `[]`. The Rust vectors remain ergonomic but
their serialization preserves that measured distinction.

## Fail-closed boundaries and comparison modes

- No fallback after failed authentication, invalid JSON, or an existing corrupt
  canonical file. Alternate-basename discovery occurs only for a missing path;
  permission/other stat errors do not redirect to a different identity.
- Key lookup uses the existing read-only resolver, after header parsing/v0
  rejection and before nonce/decrypt. Errors retain no filename, JSON excerpt,
  plaintext, or key. Profile types intentionally do not implement `Debug`.
- Decrypted temporary bytes are zeroized. Special files are rejected; Unix
  opens are nonblocking and the opened handle is rechecked before reading.
  This is not a filesystem sandbox or a trusted-root confinement claim.
- Go's short-nonce input panics; the capture retains that observed failure as
  `nonce_panic`. Rust returns `ProfileError::Nonce` instead. It does not reproduce
  an oracle crash or accept unauthenticated data.
- Profile contents, existence, key reads, and no-write snapshots are compared.
  Stable errors (missing profile/key, separator, v0, authentication) are exact.
  Parser/nonce/OS errors compare typed outcomes; raw Go excerpts are deliberately
  redacted. Native stat/read OS prose is excluded from cross-platform equality.
  This is not a claim of byte-identical CLI diagnostics for every malformed input.
- The parser uses bounded-depth serde JSON decoding. Invalid UTF-8/lone Unicode
  surrogates are rejected rather than claiming Go's replacement-character
  permissiveness; full arbitrary-JSON decoder equivalence is not claimed.

## Reproduction

From this checkout:

    python3 rust-tests/parity/profile_read.py --rust --mutation-check
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test -p symeraseme-core --lib identity --locked

Deliberate fixture refresh only:

    python3 rust-tests/parity/profile_read.py --generate --rust

Mutation mode changes the captured full-name expectation, requires the actual
Rust comparator to fail specifically on that value, restores the exact original
bytes in a finally block, and requires a successful rerun. Normal verification
never updates expected output. Native CI runs this same command on Linux,
macOS, and Windows. Dispatch `rust-ci.yml` with `profile_read_only=true` for the
focused runtime lane; formatting and workspace all-target strict Clippy still
run. Execution at the exact candidate SHA is required; the existence of this
workflow is not native PASS evidence.

For this implementation milestone, operator-approved Windows acceptance is
deferred to the final consumer phase before overall completion/release/cutover.
The focused dispatch runs Go/Rust runtime and mutation gates on macOS/Linux and
an all-target core compile check on Windows, not Windows Clippy/runtime tests.
The default full workflow retains Windows runtime coverage. Record
`windows_acceptance=deferred`; no all-platform matrix row is promoted by this
milestone, and envelope/key/path/integrity defects are not eligible for deferral.
