# Go scheduler integration

Historical Go implementation notes. The Go package described below has been
removed from the current source tree; its code remains in immutable Git history.
The current Rust scheduler lives in
[`crates/symeraseme-engine/src/scheduler/`](../../crates/symeraseme-engine/src/scheduler/),
with generation and lifecycle coverage in `scheduler_parity.rs` and
`scheduler_install_parity.rs` under `crates/symeraseme-engine/tests/`.
Run `make rust-gate` from the repository root for the current Rust suite.

`internal/scheduler` provides the cross-platform scheduler integration without relying on
ambient `PATH` resolution. An empty `Config.BinaryPath` resolves to the exact
absolute path returned by `os.Executable`; an explicit path is preserved
verbatim in generated wrappers.

Supported backends are:

- cron: daily tick, hourly poll wrapper, and quarterly dry-run re-scan;
- macOS launchd: three deterministic user LaunchAgent plists;
- Linux systemd: three deterministic user service/timer pairs.

`Generate` is deterministic for a fixed configuration. `WriteFiles` writes
sorted, path-safe output and makes shell wrappers executable. `Install`,
`Status`, and `Uninstall` provide native user-level lifecycle helpers and use a
command runner seam for tests.

## Upgrade safety

Before installing launchd or systemd units, `Install` scans the native user
unit directory. Existing Symaira unit names are returned as replacement
candidates, and installation returns `ErrLegacyUnits` unless
`InstallOptions.ReplaceLegacy` is explicitly true. Legacy markers are reported
on `LegacyUnit` so the Go scheduler never silently overwrites an existing
schedule without consent.

Cron schedules use the marked `# Symaira EraseMe scheduled tasks` block and are
replaced only by the cron lifecycle helper; unrelated crontab entries are
preserved.

## Verification limits

The package tests exercise generation, XML/unit content, path quoting,
filesystem installation, legacy detection, replacement refusal, and command
selection using temporary directories and a recording runner. They do not claim
runtime verification of `launchctl`, `systemctl --user`, or `crontab`: those
platform daemons and user sessions are environment-specific and are not
available as a deterministic acceptance dependency in this worktree.
