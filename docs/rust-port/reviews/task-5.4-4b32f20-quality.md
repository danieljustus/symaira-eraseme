# Task 5.4 quality/security review (cycle 1)

- PR: #907
- Task: 5.4 — Port scheduler file generation (`Generate`/`WriteFiles` slice; DOM-009)
- Reviewed SHA: `4b32f204`
- Reviewer lane: quality/security (Rust engineering, safety; not Go-parity)
- Verdict: **CHANGES-REQUESTED**

## Commands and results

| Command | Result |
|---|---|
| `cargo clippy -p symeraseme-engine --all-targets --all-features -- -D warnings` | PASS (0 warnings) |
| Standalone `rustc` repro proving a live path-traversal write via `write_files` | reproduced the bug (see finding 1) |
| `#![deny(unsafe_code)]` presence check | PASS, unchanged |
| Scan of all production code for `.unwrap()`/`.expect()`/panicking indexing on `Config`-derived input | PASS, none found (only one infallible `write!` to a `String`) |
| Null-byte-in-filename behavior check | PASS, surfaces as `io::Error`, no panic |

## Findings

1. **BLOCKING — path-traversal bypass in `is_safe_relative_filename`/`clean_relative`** (mod.rs, then at lines 335-349, used by `write_files` at line ~322). Built and ran a proof of concept against the actual `pub fn write_files`: inserting key `"foo/../../outside.txt"` wrote attacker-controlled content to `sandbox_root/../outside.txt` — **one directory above the caller's `output_dir`** — and the call returned `Ok`, not `Err`. Confirmed regression from Go's correctly-rejecting `filepath.Clean`-based check. `write_files`'s own doc comment states the invariant this breaks ("Only relative paths are accepted, preventing a caller from escaping `output_dir`"), and the function is `pub`, slated to become CLI-reachable in Task 8.4.
2. **BLOCKING — Windows-specific bypass via `is_absolute()`** (mod.rs, then at line 336). Grounded in the stable `std::path` doc contract: on Windows, `is_absolute()` requires *both* a prefix and a root, so a rooted-no-prefix name (`\Windows\System32\evil.sh`) and a drive-relative-no-root name (`C:temp\evil.sh`) both report `is_absolute() == false` and would be accepted, then resolve outside `output_dir` via `PathBuf::join`'s documented root/prefix-replacement semantics. Not runtime-verified on an actual Windows host (none available in this environment); reasoning is grounded directly in `std::path`'s documented, stable API contract.
3. **should-fix — TOCTOU window in `write_with_mode`** (mod.rs, then at lines 351-363). `fs::write` then a separate `fs::set_permissions` leaves a narrow window where the file exists at its final path with a mode the code didn't intend. Low severity here (scheduler script contents are paths, not secrets), but the codebase already has the correct pattern for this class of problem in `crates/symeraseme-core/src/identity/consent.rs` (`atomic_write`: write to a `NamedTempFile`, tighten its permissions, then `.persist()`), which this module didn't reuse.
4. **should-fix — thin `SchedulerError` test coverage**. Only 2 of 8 variants had dedicated tests at review time; notably, no test covered the *multi-component* traversal case that would have caught finding 1 directly (only the trivial single-component `"../escape"` case was tested).
5. Non-issues (verified, not just trusted): Clippy clean; `#![deny(unsafe_code)]` intact; no panics on adversarial `Config` input; null-byte filenames surface as `io::Error` not a panic; `SchedulerError::source()` wiring correct for all `io::Error`-wrapping variants; `[String; 29]` array in `launchd.rs::plist()` is compile-time length-checked (not a silent-drift risk), just a slightly heavy idiom (nit); `which_systemctl`'s single `PATH` walk is not a realistic DoS vector at CLI-invocation time.

## Resolution

Findings 1, 2 and 3 fixed in commit `d2c15190` (shared `lexically_clean` helper; `has_root()` + `Prefix` component check replacing bare `is_absolute()`; `write_with_mode` rewritten to use `tempfile::NamedTempFile` + `persist`, mirroring `consent.rs`'s `atomic_write`). Finding 4 addressed with new tests covering the exact bypass shape end-to-end, `EmptyOutputDirectory`, `CreateOutputDirectory`, and the TOCTOU fix. See cycle-2 verification: `task-5.4-d2c1519-verify.md`.
