# Migration JSON-state oracle (CLI-024)

This family captures **production `migration.Run`**, extracted without edits from
`4e582f284a639bdaa01260b6b8ab6000482e6c0d`. The generator verifies that the
candidate's `internal/migration/migration.go` still has those exact bytes. Its
only production dependency, `internal/scheduler/scheduler.go`, is extracted from
the same immutable commit. Both packages use only the standard library here;
a disposable module avoids fetching unrelated repository dependencies.

`fixture.json` contains 500 named, language-neutral byte inputs (hex), raw Go
stdout (hex), and the corresponding decoded observations. These are real Go
results for synthetic disposable installations, not handwritten expected errors.
The source and generator SHA-256 identities are inside the fixture. The replay
also anchors the complete capture digest independently in its provenance test:

`3b6839287256b08d74e891203ed663a88fdc9851c6779d41ed5c9397f8a84d9f`

## Reproduce

From the repository root, using the Go 1.26.6 SDK (the generator rejects other
versions):

```sh
python3 rust-tests/parity/oracle/migration-state/generate.py \
  --go /Users/daniel/sdk/go1.26.6/bin/go
python3 rust-tests/parity/oracle/migration-state/generate.py \
  --go /Users/daniel/sdk/go1.26.6/bin/go \
  --fixture "$PWD/target/json-evidence/generation-2.json"
python3 rust-tests/parity/oracle/migration-state/generate.py \
  --go /Users/daniel/sdk/go1.26.6/bin/go --check
```

Two independently executed generations were byte-identical. Check mode executes
all cases again and compares without writing the requested fixture. A disposable
copy with a changed expected error was rejected with exit 1; its bytes were
unchanged afterward. See the original named log streams in `evidence/outputs.json` and the
separate `evidence/negative-result.json`. Raw per-case stdout is retained in the fixture itself.

The probe is built with `go1.26.6 build -trimpath -o <scratch>/probe .` inside the
extracted module. Each case executes that binary with its own cwd, HOME,
USERPROFILE, XDG directories and temp directories under this worktree's `target`.
The environment is allowlisted; the probe calls no provider, fallback, keychain,
network service, or subprocess. It supplies an explicit binary path, project
path, home, and cron platform to production Run. Go builds use GOPROXY=off,
GOSUMDB=off, GOTOOLCHAIN=local, GOWORK=off and a worktree-owned build cache.
Process groups have a 60-second timeout with bounded kill/reap on the observed
POSIX host. This is an isolation harness, not an OS security sandbox.

## Comparison contract

The Rust integration test runs the actual engine in one isolated subprocess per
case. It checks nonempty, unique declarations and exact declared/executed ID
set equality. Each child must report one executed, passing test. The parent
compares the retained raw oracle output with its decoded expected object too.
A 15-second child deadline and file-backed stdout/stderr prevent hangs from
large assertion diagnostics; the engine operation does not spawn descendants.

Compared fields are exact error text, resumed/complete flags, whether the backup
was reported, item statuses, and the entire case-root filesystem before and
after (directory membership and file bytes, excluding the synthetic HOME).
This includes unexpected sibling backup directories, destination sentinels,
unchanged malformed state and marker bytes, newly written state, and backup
contents. Only the JSON-escaped temporary source/destination/backup path strings
are replaced by `@ROOT@/source`, `@ROOT@/destination`, and `@ROOT@/backup`.
Errors, CRLF, invalid Unicode, and malformed diagnostics are never normalized.
Modes and timestamps are outside this JSON-specific corpus's comparison.

The corpus covers empty/truncated/malformed/trailing JSON, CRLF and multiline
errors, every possible single input byte, top-level null and wrong types,
omitted/null/wrong-type state fields, signed version boundaries, ASCII and
Unicode-folded field names, duplicate fields with nulls and prior type errors,
items map merging/clearing and null elements, unknown nested values, invalid
UTF-8, isolated/pair UTF-16 surrogates, HTML and line-separator serialization,
completion-marker validation and resumption, and the 10,000-container limit.

## Decoder scope

`src/migration/json.rs` is private to these two migration documents. A flat,
iterative token tape validates syntax first, preserving Go's error precedence
and exact byte diagnostics while keeping duplicate fields in input order.
Assignment then implements only the production structs' integer/string/map
fields. Unknown values are skipped without converting numbers to floating point.
Returning the first assignment error is equivalent here because both Go callers
discard the decoded state on any error. Go's scalar-null no-op and map-element
zero-value semantics are intentionally different.

Serde's ordinary derive/Value path cannot retain duplicate-field history,
produce the required syntax diagnostics, accept Go's malformed Unicode, or
accept the Go depth limit. A private std-only scanner avoids changing shared
JSON behavior or adding dependencies. State serialization still uses the
existing Serde encoder. All path, backup, atomic-write and native Path/OsString
code remains unchanged.

## Observed gates and remaining scope

`evidence/identity.json` binds the dirty candidate source hashes, replay binary,
worktree, base, branch, compiler and observed counts. `evidence/outputs.json`
retains exact UTF-8 log bytes and checksums (avoiding the repository-wide
`*.log` ignore rule). `evidence/run-gates.py`
records the exact isolated environment and commands; `gates.json` records exits.
The focused command was:

```sh
CARGO_TARGET_DIR="$PWD/target" cargo test --manifest-path "$PWD/Cargo.toml" \
  -p symeraseme-engine --test migration_json --locked --offline -- --nocapture
```

Observed on **darwin/arm64**, Cargo/Rust 1.98.0 and Go 1.26.6:

- Focused: 2 tests passed; 500 declared = 500 executed cases.
- Whole engine: 42 tests passed (17 unit, 14 migration, 2 JSON, 3 scheduler
  install, 6 scheduler); the Linux-only native-name binary and doctests each
  selected zero tests, explicitly not counted as platform evidence.
- Engine and CLI Clippy, all targets and all features, `-D warnings`: passed.
- Workspace `cargo fmt -- --check` and `git diff --check`: passed.
- Cargo test/metadata/Clippy used explicit manifest, isolated target, locked and
  offline flags; fmt has no locked/offline flags and ran with CARGO_NET_OFFLINE.
- Metadata asserted the assigned workspace root, target directory, selected
  manifests and all selected target source paths.

No mismatches remain in the captured 500 cases. Explicit unverified scope:

1. Native Linux and Windows execution (including Linux non-UTF-8 path tests).
   The Python generator's process-group timeout is POSIX-specific.
2. 32-bit Go/Rust `int` targets; this capture was produced on 64-bit arm64.
3. Exhaustive arbitrary JSON combinations and allocation-failure behavior for
   enormous inputs. The token tape uses memory proportional to token count;
   this work does not assert Go-equivalent resource consumption.
4. The parent's combined-workspace integration gate.

`dev-external --status` was attempted but its shared lock was denied by the
sandbox. The external volume and this worktree-local target were present; no
internal cache directory was created. There were no commits, GitHub writes,
new dependencies, or installed application-data accesses.
