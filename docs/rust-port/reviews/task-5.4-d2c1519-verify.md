# Task 5.4 fix verification (cycle 2)

- PR: #907
- Task: 5.4 — Port scheduler file generation (`Generate`/`WriteFiles` slice; DOM-009)
- Reviewed SHA: `d2c15190` (fix commit; test-coverage follow-up landed separately at `7840464`)
- Reviewer lane: independent re-verification of the cycle-1 spec + quality findings
- Verdict: **PASS**

## Commands and results

| Command | Result |
|---|---|
| `cargo test -p symeraseme-engine` | PASS (13 unit + 2 parity tests at time of review; 16 unit tests after the `7840464` follow-up) |
| `cargo clippy -p symeraseme-engine --all-targets --all-features -- -D warnings` | PASS |
| `cargo deny check` | advisories/bans/licenses/sources all ok |
| Independent hand-trace + compiled throwaway repro of `lexically_clean` against Go `filepath.Clean` for 9+ inputs, including the untested-at-the-time `"a/b/../../c"` deep-resolvable case | all match Go exactly |
| Adversarial bypass hunt (`"."`, trailing slash, `"a/../../../b"`, `"a//../../b"`, Windows prefix-variant reasoning) | no new bypass found |
| `tempfile` 3.27.0 source review (`persist`/`NamedTempFile::new_in`) for the TOCTOU fix | atomic rename on POSIX confirmed, same-filesystem co-location confirmed, failure path returns `Err` not a panic |

## Results

Each cycle-1 finding re-derived independently, not taken on the fix commit's word:

1. **Traversal bypass** — FIXED. `lexically_clean` correctly collapses `..` against a preceding `Normal` component and drops it at a root/prefix; `is_safe_relative_filename` now runs it before the `starts_with("..")` check.
2. **Windows path check** — FIXED (reasoned from `std::path::Prefix` semantics; no Windows host available in this environment, so the `#[cfg(windows)]` test itself has not executed in CI yet — flagged as a residual verify-on-native-Windows-CI item, not a code defect).
3. **`which_systemctl` executable bit** — FIXED, matches Go's `exec.LookPath` bit-for-bit including symlink resolution via `fs::metadata`.
4. **TOCTOU in `write_with_mode`** — FIXED, atomic rename confirmed from the `tempfile` crate source.
5. **Thin `SchedulerError` test coverage** — was partially closed as of `d2c15190` (2 more variants covered); fully closed for every *practically testable* variant by the follow-up commit `7840464` (`WriteFile` via a read-only directory). `UnsupportedPlatform` stays CLI-layer territory (Task 8.4) and `ResolveBinaryPath`/`ResolveProjectDirectory`/`ResolveOutputDirectory` stay untested as genuinely impractical-to-simulate OS-failure paths, consistent with house convention elsewhere in this codebase for that class of error.

No new bypass or regression found under adversarial testing. `rust_generate_matches_frozen_go_fixture_byte_for_byte` and `frozen_fixture_matches_live_go_oracle` both still pass, confirming the fix changed no observable `Generate()` output — it is validation-logic-only, as intended.

## Outstanding

- The `#[cfg(windows)]` traversal test needs to actually execute on native Windows CI at least once before this slice's Windows-path-safety claim is fully proven end to end (currently proven by source-level reasoning only). Native CI runs on this PR should cover that.
