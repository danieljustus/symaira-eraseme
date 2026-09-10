# Task 5.4 specification review (cycle 1)

- PR: #907
- Task: 5.4 — Port scheduler file generation (`Generate`/`WriteFiles` slice; DOM-009)
- Reviewed SHA: `4b32f204`
- Reviewer lane: specification (byte-exact Go parity, scope honesty)
- Verdict: **FAIL-WITH-FINDINGS**

## Commands and results

| Command | Result |
|---|---|
| `cargo test -p symeraseme-engine --test scheduler_parity` | PASS (both tests, including live-oracle rebuild) |
| `cargo test -p symeraseme-engine` (unit tests) | PASS (8/8) |
| Manual line-by-line comparison of `internal/scheduler/scheduler.go` (lines 1-514) against `crates/symeraseme-engine/src/scheduler/{mod,cron,launchd,systemd}.rs` | see findings |
| Independent `go run` / `rustc` repros of `filepath.Clean`-equivalent behavior | see finding 2 |

## Results

- Scope claim confirmed honest: only `Generate`/`WriteFiles` are ported; everything past scheduler.go:~514 (Install/Uninstall/Status/legacy-detection) is correctly declared out of scope and not silently covered.
- All fixture-covered behavior (6 cases spanning all 3 backends, custom/default tick+poll hours, venv activation with embedded quotes, the wrapper()-vs-poll_wrapper() blank-line asymmetry after sourcing venv) is byte-exact against the Go oracle.
- Oracle methodology sound: `rust-tests/parity/oracle/scheduler/main.go` calls the real unimported `internal/scheduler` package; provenance SHA/revision constants verified independently against `git log` / `shasum -a 256` and match.
- Case-config sync between `scheduler_parity.rs::rust_case_config` and `main.go`'s `cases` map verified field-by-field for all 6 cases — no mismatch.

## Findings

1. **`detect_platform()`/`which_systemctl()` — missing executable-bit check** (mod.rs, then at lines 69-74). Severity: Medium. Go's `exec.LookPath` requires the PATH candidate to be executable, not merely present; the Rust port only checked `.is_file()`. In-scope code, zero fixture coverage (all 6 cases pin an explicit platform).
2. **`is_safe_relative_filename`/`clean_relative` — traversal-validation bypass for multi-segment names** (mod.rs, then at lines 335-349). Severity: Medium (logic), Low (practical reach at the time, since `write_files` had no caller). Rust rendered path components without lexically collapsing `..` against a preceding `Normal` component before validating, so e.g. `"a/../../b"` — which Go's `filepath.Clean`-based check correctly rejects — passed Rust's check unrejected. Confirmed by hand-tracing both `go run` and a standalone Rust repro.
3. **`clean_path` reused for the `write_files` output root — doesn't collapse `..`**. Severity: Low. Same root cause as #2, cosmetic/textual only for this call site (OS path resolution collapses `..` transparently at write time; no traversal-outside-root consequence since this is the trusted root, not an attacker-supplied relative name).
4. Platform-name lowercasing relocation to `Platform::parse` and Windows chmod-no-op — reviewed, both are non-issues (intentional/equivalent consequences of the Rust type design).

## Resolution

All findings addressed in commit `d2c15190` (see cycle-2 verification: `task-5.4-d2c1519-verify.md`). Findings 1-3 fixed via a new shared `lexically_clean` helper (real `..`-collapse, matching `filepath.Clean` including the at-root-drop case) and an executable-bit check in `which_systemctl`.
