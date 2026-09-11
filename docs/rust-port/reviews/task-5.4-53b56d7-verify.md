# Task 5.4 fix verification (cycle 2)

- PR: (direct-push, no PR)
- Task: 5.4 — Port scheduler legacy-Python-unit detection (`isPythonSchedulerContent`,
  `DetectLegacyPythonUnit`, `DetectLegacyPythonUnits`, `ScanLegacyPythonUnits`,
  `LegacyUnit`; DOM-009)
- Reviewed SHA: `53b56d7f` (fix commit for `task-5.4-5ea2eff-spec.md`; branch
  `agent/task5.4-scheduler-legacy-detection`, HEAD)
- Reviewer lane: independent re-verification of the cycle-1 specification findings
- Verdict: **PASS**

## Commands and results

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS (no diff) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS (clean) |
| `cargo test -p symeraseme-engine --test scheduler_parity` | PASS (6/6, incl. `frozen_fixture_matches_live_go_oracle` and `rust_detect_legacy_python_unit_matches_frozen_go_fixture`) |
| `cargo test --workspace` | PASS on rerun. One transient failure on first run (`run_file_backed_enforces_live_output_limit` in `symeraseme-core`'s `config_parity.rs`: `oracle process-tree cleanup failed: Operation not permitted (os error 1)`) reproduced as an environment/sandbox signal-permission flake — passes in isolation and on a clean rerun; unrelated file (`crates/symeraseme-core/tests/config_parity.rs`, oracle-runner process-group cleanup), untouched by `53b56d7f`/`5ea2eff`/`20d9fba3`. Not a regression from this fix. |
| `cargo deny check` | advisories/bans/licenses/sources all ok |
| `go build ./...` | PASS |
| `go vet ./...` | PASS |
| `go test -count=1 ./internal/scheduler/... ./rust-tests/...` | PASS |
| Independent `go run` repro of `strings.ToLower("VENV/BİN/ACTIVATE")` | `"venv/bin/activate"` — İ (U+0130) folds to plain ASCII `i`, matches marker |
| Independent `rustc` repro comparing `"VENV/BİN/ACTIVATE".to_lowercase()` vs `go_to_lower(...)` | full fold: `"venv/bi\u{307}n/activate"` (18 chars, marker match fails); `go_to_lower`: `"venv/bin/activate"` (17 chars, marker matches) — confirms both the pre-fix bug and the fix |
| `char::to_lowercase().next()` vs `to_ascii_lowercase()` for every char in all 5 markers + the legacy-marker string | no mismatches — fix introduces no new ASCII divergence |

## Results

Each cycle-1 finding re-derived independently against `53b56d7f`, not taken on the fix commit's word:

1. **Stale contract matrix (Medium)** — FIXED. `docs/rust-port-contract-matrix.md`'s DOM-009 row now reads "...legacy-Python unit detection (`DetectLegacyPythonUnit`/`DetectLegacyPythonUnits`/`ScanLegacyPythonUnits`) byte-exact against the same oracle, reviewed and fixed (task 5.4 legacy-detection slice); TODO: Install/Uninstall/Status, native macOS/Linux/Windows execution of the Windows-specific traversal test" — "legacy-Python detection" is gone from the TODO clause, the PASS clause names the three actually-ported functions against the actually-used oracle (`rust-tests/parity/oracle/scheduler`), and Install/Uninstall/Status correctly remain in TODO.
2. **Unicode case-folding divergence, U+0130 (Low)** — FIXED, verified independently rather than trusted. `go_to_lower` (new, `crates/symeraseme-engine/src/scheduler/mod.rs:494`) takes only `c.to_lowercase().next()` per char. Confirmed by direct `rustc` repro: Rust's *full* `to_lowercase()` on `"VENV/BİN/ACTIVATE"` inserts a combining dot-above (U+0307) after the folded `i`, breaking the `venv/bin/activate` substring match; `go_to_lower` recovers the one-rune-out simple mapping and the match succeeds — matching `strings.ToLower`'s actual behavior, independently confirmed with a standalone `go run`. The doc comment's reasoning ("Unicode's full mappings only ever append combining/ligature components after the base simple-mapped character") holds: U+0130 is in fact the *only* unconditional (locale-independent) multi-rune entry in Unicode's default lowercasing table, so there is no other character for which this base-then-append property could fail. A real oracle fixture case was added, not a Rust-only test: `rust-tests/parity/oracle/scheduler/main.go` gained `legacyContent["turkish_dotted_i_marker"] = "ExecStart=/bin/bash VENV/BİN/ACTIVATE\n"`, and `scheduler_cases.json` was regenerated with `"turkish_dotted_i_marker": {..., "is_python": true}` — confirmed present in the file. `rust_detect_legacy_python_unit_matches_frozen_go_fixture` iterates every entry of `frozen_legacy_fixture()` (parsed straight from the same JSON file), so it picked up the new case with no Rust-test-file edit, as claimed — traced the loop in `scheduler_parity.rs:197-212` to confirm. All 6 `scheduler_parity` tests pass, including the live-oracle rebuild test. Checked for a new divergence introduced by the "first char" strategy: every character across all 5 ASCII markers and the legacy-marker string produces identical output from `c.to_lowercase().next()` and `c.to_ascii_lowercase()` — no regression for any marker-relevant input.
3. **Byte vs lossy-UTF8 boundary divergence (Low)** — NOT silently ignored; documented and justified, and the disposition holds up. The new doc comment on `is_python_scheduler_content` (`mod.rs:505-513`) explicitly states the limitation and its justification. Independently checked all current callers of `is_python_scheduler_content`/`detect_legacy_python_unit(s)`/`scan_legacy_python_units` in both languages (`grep` across `crates/` and `internal/`): in Rust, none exist outside `scheduler::` itself — no CLI wiring yet. In Go, the only callers are `Install`/`Status` (not yet ported; `Status`'s only non-file input is `crontab -l` output) and `internal/migration`, all reading scheduler-generated unit files, `crontab` output, or legacy-Python-installer output — never attacker-controlled binary input. This is a reasonable, low-severity, currently-unreachable finding with a documented, reasoned acceptance rather than a forced byte-level rewrite; no evidence found of a realistic scenario where these paths see arbitrary binary content.
4. **Informational, deferred (future Install slice, typed-vs-string Platform)** — correctly still out of scope. `git diff 5ea2efff..53b56d7f` touches only `mod.rs` (the two doc comments + `go_to_lower`), `docs/rust-port-contract-matrix.md`, and the oracle fixture pair — no `Platform`/CLI-flag-parsing code was touched, consistent with this remaining deferred to the not-yet-started Install slice.

The unrelated intermediate commit `20d9fba3` (`.gitattributes` `*.go text eol=lf`, fixing a real, independently-discovered Windows CRLF/oracle-hash CI break) required no action and received none from `53b56d7f`.

No new bypass or regression found under adversarial review of the fix's scope. All local gates are green (modulo one confirmed-transient, unrelated sandbox flake on the first `cargo test --workspace` run, which passed clean in isolation and on rerun).

## Outstanding

None specific to this slice. Carried forward from cycle 1 and still correctly deferred: Install/Uninstall/Status (DOM-009 TODO), and the future Install slice's `Platform::parse` "unrecognized value" behavior decision (cycle-1 finding 4).
