# CLI-014 — scheduler install / status / uninstall

## What landed

The three scheduler CLI operations leave the deferred stub and are ported to Rust
byte-exactly against a committed Go capture.

- `crates/symeraseme-engine/src/scheduler/install.rs` — `install`, `status`,
  `uninstall` plus the `Runner` seam (`ExecRunner` in production, a recording
  fake in tests), `InstallOptions`, `InstallResult`, `StatusResult`/`StatusEntry`
  and the two Go behaviours of #1000.
- `crates/symeraseme-engine/src/scheduler/mod.rs` — `Platform::from_name` (Go's
  platform validation on the string form: empty auto-detects, unknown is
  rejected), the `UnsupportedPlatformOperation` variant, `working_directory`
  (Go's `os.Getwd`, which prefers `$PWD`), and `Serialize` for `Platform` and
  `StatusResult` with Go's field names.
- `crates/symeraseme-cli/src/cli.rs` — `schedule install`, `schedule status`,
  `schedule uninstall` dispatch arms and bodies.
- `rust-tests/parity/oracle/scheduler-install/main.go` — the engine oracle: 17
  cases driving the real `internal/scheduler` with a recording `Runner` and a
  private HOME.
- `rust-tests/parity/oracle/cli-schedule/main.go` — the CLI oracle: 12 cases
  running the real Go CLI with an empty PATH, so no host scheduler is touched.
- `crates/symeraseme-engine/tests/scheduler_install_parity.rs` — replays the 17
  engine cases; rebuilds the capture from the live oracle first and fails if the
  committed fixture drifted.
- `crates/symeraseme-cli/tests/command_surface.rs` — replays the 12 CLI cases and
  now also compares the three `operate-schedule-*` phase-two cases.

## Evidence

| Gate | Result |
|---|---|
| `cargo nextest run --workspace` | 383 passed, 2 skipped |
| `cargo clippy --workspace --all-targets --all-features --locked -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `go vet ./...`, `gofmt -l .` | clean |
| `CI=true make lint` | `0 issues` |

Both oracles are deterministic: running each twice produces identical output.

## Masked values (named, not blanket)

Exactly three volatile values are folded, and each one's format is asserted
rather than trusted:

1. **Install root** — the per-run temp directory the case uses.
2. **Resolved binary path** — the CLI embeds its own executable path into the
   generated wrappers. Folded to `<BINARY>`; the replay asserts it is absolute
   and names the CLI.
3. **`$PWD`** — Go's `os.Getwd` prefers the logical `$PWD` over the kernel's
   symlink-resolved view, so captures run with `PWD` set, as a real shell does.

Nothing else is masked: no broad JSON sorting, no whitespace trimming, no
stderr removal.

## Go defects pinned as measured (not as desired) — #1000

1. **`Status` and `Uninstall` disagree about launchd unit names.** `Status`
   resolves `symeraseme-tick.plist`; `Uninstall` removes
   `com.symeraseme.tick.plist` (as does the legacy scan). After an install,
   `status` can therefore report `installed: false` for units uninstall removes.
   Both halves are pinned by dedicated cases.
2. **A second `install` refuses to run.** `Install` writes
   `com.symeraseme.<name>.plist`, and its own legacy scan flags exactly those
   names as Python-era units, so re-installing without `--replace-legacy` fails
   with `legacy scheduler units detected; replacement was not requested`.

Fixing either changes the Go oracle, so it is a contract change, not a port fix.

## Deliberate boundaries

- `schedule status` prints `success` in text form. Go discards the status payload
  unless JSON was requested; pinned as measured.
- The engine port runs every platform command through the injectable `Runner`, so
  no real `launchctl`, `systemctl` or `crontab` is executed in any test. Actual
  host execution stays Go's production path.
- The Windows-specific traversal case still runs only on Windows.