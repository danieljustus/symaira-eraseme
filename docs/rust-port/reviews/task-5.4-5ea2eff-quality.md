# Task 5.4 quality/security review (cycle 1)

- PR: none yet (branch `agent/task5.4-scheduler-legacy-detection`, HEAD)
- Task: 5.4 — Port scheduler legacy-Python-unit detection (`isPythonSchedulerContent`,
  `DetectLegacyPythonUnit`, `DetectLegacyPythonUnits`, `ScanLegacyPythonUnits`,
  `LegacyUnit`; DOM-009)
- Reviewed SHA: `5ea2efff` (`5ea2eff`)
- Reviewer lane: quality/security (Rust engineering, safety; not Go-parity)
- Verdict: **APPROVED**

## Commands and results

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy -p symeraseme-engine --all-targets --all-features -- -D warnings` | PASS (0 warnings) |
| `cargo test -p symeraseme-engine` | PASS (14 unit + 6 integration + 0 doc = 20/20) |
| `cargo deny check` | PASS (advisories ok, bans ok, licenses ok, sources ok) |
| `#![deny(unsafe_code)]` presence check + diff scan for `unsafe` | PASS, intact, no `unsafe` in the diff |
| Standalone `rustc` repro: `fs::read` on a directory at a legacy-unit-shaped path | `Err` `IsADirectory` (os error 21), no panic |
| Standalone `rustc` repro: `fs::read` on a 0-permission file | `Err` `PermissionDenied` (os error 13), no panic |
| Go-vs-Rust source diff of `isPythonSchedulerContent`/`DetectLegacyPythonUnit(s)`/`ScanLegacyPythonUnits` (`internal/scheduler/scheduler.go` lines 516-632) | Confirmed line-for-line matching trust boundary and marker logic |

## Findings

No blocking or should-fix findings. Three informational, non-blocking observations:

1. **INFORMATIONAL — unbounded read on huge/never-EOF files is shared with Go, not a Rust regression.** `is_python_scheduler_content` is reached via `fs::read`, which fully buffers the file (metadata-hinted capacity, then a growing read loop for streams/special files). A legacy unit path replaced with a symlink to an infinite-stream device (e.g. `/dev/zero`) would grow memory unboundedly / hang, same as Go's `os.ReadFile` would. Exploiting this requires the ability to write into `~/Library/LaunchAgents` or `~/.config/systemd/user` under the resolved home directory — i.e. same-user or already-compromised local write access, at which point the attacker already has materially broader capability than this read path grants. Matches Go's existing trust boundary; not a new risk introduced by the port. No action requested for this slice.
2. **INFORMATIONAL — no permission-denied test for the new `ReadLegacyUnit`/io error path.** `detect_legacy_python_unit_treats_missing_file_as_not_python` covers the `NotFound → Ok(false)` branch, but no test drives a 0-permission file through `scan_legacy_python_units` or `detect_legacy_python_unit` to exercise the `Err` propagation path end-to-end. Go's own `TestLegacyPythonDetection`/`TestScanLegacyUnitsAcrossPlatforms` have the identical gap (grepped `internal/scheduler/scheduler_test.go`, no `PermissionDenied`/`Chmod`/`0o000` hits), so this is parity, not a regression, and an acceptable gap for this slice size. Verified manually via standalone `rustc` repro (see table) that the error path is a graceful `Err`, not a panic, either way.
3. **NIT — return-type shape differs between the three new functions.** `detect_legacy_python_unit`/`detect_legacy_python_units` return `io::Result<_>` while `scan_legacy_python_units` returns `Result<_, SchedulerError>`. This mirrors Go's own inconsistency (`DetectLegacyPythonUnit`/`DetectLegacyPythonUnits` return a plain `error`; `ScanLegacyPythonUnits` wraps failures via `fmt.Errorf`), so it's an intentional, matching asymmetry rather than an oversight. No security impact either way — cosmetic only.

## Verified, not just trusted

- **Unsafe code**: no `unsafe` anywhere in the diff; `#![deny(unsafe_code)]` unchanged at `crates/symeraseme-engine/src/lib.rs:1`.
- **Panics on untrusted content**: `is_python_scheduler_content` operates on `String::from_utf8_lossy(&bytes)` output (`Cow<str>`, lossless-safe replacement of invalid UTF-8, no panic) before `.to_lowercase()`/`.contains()`; both are on the resulting valid `str`, no panic path. Directory-read and permission-denied cases confirmed to surface as `Err`, not a panic (see repro table).
- **Path/filesystem safety**: `scan_legacy_python_units` builds `home_dir.join("Library").join("LaunchAgents")` / `.join(".config").join("systemd").join("user")` then `.join(name)` where `name` comes only from the fixed, non-`pub` `LEGACY_NAMES: [&str; 9]` array — never from caller input beyond the `home` string itself, exactly matching Go's `filepath.Join(home, "Library", "LaunchAgents")` + `legacyNames[:3]`/`legacyNames[3:]` slicing (verified array contents and slice boundaries match 1:1, including the 3/6 launchd/systemd split). `detect_legacy_python_units` takes a caller-supplied `&[PathBuf]` directly with no validation, matching Go's `DetectLegacyPythonUnits([]string)` exactly — an intentional, unchanged trust boundary (this is a convenience scanner over paths the caller already resolved, e.g. from install-time bookkeeping, not user-facing free-text input).
- **Symlink handling**: `fs::read` uses `File::open`, which follows symlinks by default (no `O_NOFOLLOW`), identical to Go's `os.ReadFile`/`os.Open`. No TOCTOU concern: the code goes straight to `fs::read` and matches `ErrorKind::NotFound` in the result rather than doing a separate `exists()` check before reading, so there's no check-then-act race window (same pattern Go uses: `ReadFile` then `os.IsNotExist(err)`).
- **Error hygiene**: `SchedulerError::ResolveHomeDirectory` displays `"resolve home directory: {source}"` (an `io::Error` whose message is `"$HOME is not defined"` when unset — no content, no secret); `ReadLegacyUnit` displays `"read legacy unit {path}: {source}"` — path plus the OS error string (e.g. `Permission denied (os error 13)`), never file contents. Both variants added to the `source()` match arm alongside the pre-existing io-wrapping variants; nothing silently swallowed.
- **`resolve_home_dir`**: `std::env::var_os(HOME_ENV_VAR)` returns `Option<OsString>`; `PathBuf::from(OsString)` uses the lossless, infallible `From<OsString> for PathBuf` conversion — no UTF-8 validation, hence no panic on non-UTF8 env values. `HOME_ENV_VAR` is `USERPROFILE` under `#[cfg(windows)]` and `HOME` otherwise, matching Go's `os.UserHomeDir()` (which also reads `USERPROFILE` only on Windows and `HOME` on Unix, erroring when unset/empty) — confirmed against Go's documented behavior.
- **Test coverage**: missing-file → `Ok(false)` is tested (`detect_legacy_python_unit_treats_missing_file_as_not_python`); non-Python-content filtering and multi-path scanning are tested (`detect_legacy_python_units_filters_non_python_and_missing_paths`, mirroring Go's `TestLegacyPythonDetection`); per-platform scanning is tested (`scan_legacy_python_units_reports_known_names_per_platform`, mirroring Go's `TestScanLegacyUnitsAcrossPlatforms`), including the `Platform::Cron` empty-list branch. An 11-case Go-oracle-driven parity fixture (`legacy_cases` in `scheduler_cases.json`, rebuilt and re-run live against the real Go implementation by `frozen_fixture_matches_live_go_oracle`) exercises every marker keyword, case-insensitivity, the legacy-marker-without-`(Go)`-suffix edge case, the Go-generated-wrapper negative case, and empty content — all passing byte-for-byte against the Go oracle.
- **Dependency change**: `serde` added to `crates/symeraseme-engine/Cargo.toml` under `[dev-dependencies]` as `serde.workspace = true`, resolving to the workspace's single pinned `serde = { version = "1.0", features = ["derive"] }` (root `Cargo.toml:26`) — no version-family fragmentation, dev-only scope confirmed (cannot leak into production builds), `Cargo.lock` updated accordingly. `cargo deny check` clean.

## Resolution

No fixes required this cycle.
