# HTTP fixture and oracle continuation, 2026-10-03

## Starting point and scope

Continue the existing work from GitHub without local reports, a previous chat, installed agent skills or the original workstation.

| Required unit | Remote branch | Exact code revision | Pull request |
| --- | --- | --- | --- |
| Current HTTP fixture candidate and this handoff | `agent/http-reset-read-1144` | `43c369e681e7c9124c1f7467c7b36f59b98a721c` | [#1145](https://github.com/danieljustus/symaira-eraseme/pull/1145) |
| Windows executable-oracle cleanup | `agent/windows-oracle-cleanup-1138` | `60b994c55929f1918683064110ef3030e07076cd` | [#1139](https://github.com/danieljustus/symaira-eraseme/pull/1139) |
| LLM deadline diagnostics, not a timeout repair | `agent/llm-deadline-diagnosis-1142` | `55e6d05e41d46f7237a9b899bd2e7417a745a496` | [#1143](https://github.com/danieljustus/symaira-eraseme/pull/1143) |

Repository: `https://github.com/danieljustus/symaira-eraseme.git`. Integrated base: `ff75d89716bc948185c7cb38e9ff43259e0081d4`, the normal squash merge of [#1141](https://github.com/danieljustus/symaira-eraseme/pull/1141) for [#1140](https://github.com/danieljustus/symaira-eraseme/issues/1140). The current branch contains that Go compiler temporary-root repair. The older diagnostics branch does not; do not replace the integrated base with its older tree.

The revision above is the base **code** commit, not a circular reference to the commit containing this document. Obtain the final handoff HEAD from the remote branch and the publication record, verify it, and keep code acceptance separate from documentation-only commits.

This is a secured unfinished checkpoint. All three PRs remain open drafts. Publication does not authorize automatic promotion, merge, release, a tag, destructive Go retirement or production operations. This handoff closes the local session, not those issues.

## What changed and what actually works

The current HTTP patch changes only:

- `crates/symeraseme-cli/tests/support/mcp_http_port.rs`: shared nonblocking response capture, absolute deadline and 64 KiB limit. Darwin can reject read-timeout option changes after reset with EINVAL even when response bytes remain buffered. The reader avoids that option setter during capture.
- `crates/symeraseme-cli/tests/mcp_http_headers_process.rs`: strict complete header/body differential comparison, typed closure diagnostics, real FIN/truncation controls, Unix buffered-reset controls, deadline/exact-size/overflow control and complete request staging before transmission.

The Go reference rejects Origin/authentication at `internal/mcp/server.go:75-81` before reading the body. The original client sent the header terminator and body separately. The current client constructs the same bytes before network transmission, following the authenticated readiness helper's existing pattern. This removes that split-write window. One `write_all` is **not** a guarantee of one kernel write or TCP packet.

A Tokio-readiness-only experiment at `798a7f0e0270e20b35a427e870d6c9754e3cc317` failed native Windows ARM64: Go rejected-Origin case 2 reset with 0 captured bytes. It was reverted. The final stdlib reader is byte-identical to the independently inspected reader at `a085707b58bac61278529a6b2cbcf329a5910cc4`; new request staging and the size/deadline test still require final independent review.

Production code, request/response bytes, authentication, strict parser/equality assertions, oracle provenance, deadlines and dependency declarations are unchanged by the HTTP fix. Reset is a capture termination, never a substitute for complete response validation. Windows is not assumed to preserve unread bytes after reset.

## Verification and retained failures

### Current code revision

[Native run 37106329046](https://github.com/danieljustus/symaira-eraseme/actions/runs/37106329046) completed successfully at exact code HEAD `43c369e681e7c9124c1f7467c7b36f59b98a721c` on all six targets:

| Target | Job ID | Outcome |
| --- | --- | --- |
| macOS ARM64, `macos-14` | `111155472430` | success |
| Linux ARM64, `ubuntu-24.04-arm` | `111155472556` | success |
| macOS Intel, `macos-15-intel` | `111155472559` | success |
| Linux x64, `ubuntu-24.04` | `111155472568` | success |
| Windows x64, `windows-2025` | `111155472596` | success |
| Windows ARM64, `windows-11-arm` | `111155472628` | success |

Full completed-job logs were fetched, retained outside the repository and checked programmatically, not inferred from job names. Every target explicitly passed these five named tests:

1. `bounded_response_capture_enforces_deadline_and_size`
2. `bounded_response_capture_preserves_fin_and_native_reset_bytes`
3. `header_comparison_rejects_missing_extra_changed_and_duplicate_headers`
4. `native_http_complete_headers_and_bodies_match_go`
5. `readiness_probe_is_bounded_when_peer_dribbles_bytes`

See [native status](evidence/2026-10-03-http-fixture/native-status.json) for head/run/job/case binding, raw-log hashes and links, and [retained passing lines](evidence/2026-10-03-http-fixture/native-case-excerpts.txt). The portable FIN controls run on Windows; buffered-reset controls inside the test are Unix-only.

Local macOS verification of this code passed all 175 CLI tests in 16 test binaries, with 0 failures and 0 ignored tests, plus strict CLI all-target Clippy, workspace formatting and diff checks. The Windows-only integration binary executes 0 cases on macOS; Windows runtime evidence comes from the actual native jobs, not that local binary. The old local external-storage compilation blocker did not recur.

### Historical failures remain valid

| Revision/run | Observation and interpretation |
| --- | --- |
| `60b994c55929f1918683064110ef3030e07076cd`, run `37054779350`, job `110996683763` | macOS ARM64 failed setting a nonzero socket read timeout with EINVAL after reset. |
| `728fbcd2b48a77dda88634c52c03361d30af0ab8`, run `37058595903` | Failed the old Windows assumption that unread bytes survive reset. Removing that nonportable synthetic assumption did not weaken actual complete-response parity. |
| `2b6913eeda1f8a5f058c53349df6a0deb78c1919`, run `37061056628`, job `111017893883` | Actual Windows ARM64 response lacked the header terminator. |
| `a085707b58bac61278529a6b2cbcf329a5910cc4`, run `37066468147` | Diagnostics attributed a zero-byte reset to Go rejected-Origin case 2. |
| `798a7f0e0270e20b35a427e870d6c9754e3cc317`, run `37104549436`, job `111150420181` | Readiness-only scheduling was insufficient: Go case 2 again reset with 0 bytes. The size/deadline control passed. |
| Cleanup candidate, run `37045269185`, job `110965028842` | Windows x64 LLM fixture `context_overflow_response_matches_go_with_secret_redaction` hit error 10035 at its 15-second accept deadline. This was not an executable-oracle mismatch. |

[The failed readiness-candidate excerpt](evidence/2026-10-03-http-fixture/failed-readiness-candidate.txt) retains the failure and diagnostic context. [Darwin socket controls](evidence/2026-10-03-http-fixture/darwin-socket-control.json) are real Python POSIX observations, not Rust acceptance by themselves. [Historical reviews](evidence/2026-10-03-http-fixture/historical-reviews.md) bind only to their stated revisions; neither late static approval nor a smaller passing test subset clears broader native failures.

The diagnostics at `55e6d05e41d46f7237a9b899bd2e7417a745a496`, run `37050932501`, observed Windows provider requests/outcomes completing within roughly six seconds without changing deadlines or retry policy. They did not establish why the original 15-second failure occurred. [#1142](https://github.com/danieljustus/symaira-eraseme/issues/1142) remains open.

## Reproduction without the original machine

Work from the repository root. Use Git, Rustup/Rust/Cargo, Go, Python 3 and a native C compiler/linker. `rust-toolchain.toml` pins Rust 1.98.0 with rustfmt and Clippy. `go.mod` requires Go 1.26.6; native CI uses Go 1.26.6. The observed local launcher was Go 1.27.1. Honor exact toolchain/source bindings in oracle generators rather than regenerating fixtures with an arbitrary installed Go version.

Observed local tools: Rust/Cargo 1.98.0, Go 1.27.1 darwin/arm64, Python 3.14.2, Git 2.54.0 and gh 2.102.0. Install missing tools through the target's established tooling. If Rust is missing, the repository-documented pinned setup is `rustup toolchain install 1.98.0 --profile minimal --component rustfmt,clippy`; no new toolchain installation was necessary during this closeout.

```sh
git clone --single-branch --branch agent/http-reset-read-1144 https://github.com/danieljustus/symaira-eraseme.git
cd symaira-eraseme
git rev-parse HEAD
git ls-remote --exit-code origin refs/heads/agent/http-reset-read-1144
git status --porcelain=v1 -uall
cargo fetch --locked
cargo fmt --all --check
cargo test --locked -p symeraseme-cli --all-targets
cargo clippy --locked -p symeraseme-cli --all-targets -- -D warnings
cargo run --locked -p symeraseme-cli -- --help
git diff --check
```

These commands were actually executed on a **fresh clone from GitHub** of code revision `43c369e681e7c9124c1f7467c7b36f59b98a721c` on local macOS. Dependency fetch, formatting, the complete 175-test CLI suite, strict Clippy, help and clean status all passed. No worktree files, stash contents, untracked inputs, symlinked sources or local source/configuration overrides were copied in. An independent new build target was created by Cargo in the fresh clone; ordinary package-manager download caches were allowed. See [fresh CLI output](evidence/2026-10-03-http-fixture/fresh-checkout-cli-tests.txt), [Clippy](evidence/2026-10-03-http-fixture/fresh-checkout-clippy.txt) and [help](evidence/2026-10-03-http-fixture/fresh-checkout-help.txt).

For this fresh build only, `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_PROFILE_DEV_DEBUG=0` bounded build resources. `TMPDIR` and `GOTMPDIR` named a writable owned temporary directory. Their literal workstation paths are not inputs: use a target-local owned directory if needed. They do not replace code or fixture paths. No provider API key, password, production identity or real removal data was supplied. Scoped tests use isolated synthetic application state, real local protocol peers and the source-bound Go executable.

Build-only command: `cargo build --locked -p symeraseme-cli`. The test/help chains built and exercised the executable; a separate production/release build was not performed. Help is a safe start smoke, not proof of a running user-facing service. Do not start a broker campaign or send real email/removal requests during verification.

To inspect another pending unit without mixing trees:

```sh
git fetch origin refs/heads/agent/windows-oracle-cleanup-1138:refs/remotes/origin/agent/windows-oracle-cleanup-1138
git fetch origin refs/heads/agent/llm-deadline-diagnosis-1142:refs/remotes/origin/agent/llm-deadline-diagnosis-1142
git show origin/agent/windows-oracle-cleanup-1138:tests/test_mcp_stdio_oracle_cleanup.py
git show origin/agent/llm-deadline-diagnosis-1142:crates/symeraseme-core/tests/llm_http_failure_next.rs
```

The cleanup branch's focused commands are `python3 tests/test_mcp_stdio_oracle_cleanup.py` and `python3 tests/test_mcp_stdio_oracle_check.py`; the generator's check mode is `python3 rust-tests/parity/oracle/mcp-stdio-mutations/generate.py --check`. Earlier portable policy controls passed, and native reviews observed both real Windows handle tests and the 670-case oracle check passing. This closeout did not independently rerun that branch's whole matrix. Its failed broader acceptance remains open.

## Dependencies, disposition and cloud limits

[The input inventory](evidence/2026-10-03-http-fixture/inputs.json) classifies all 122 external artifacts found in the initial closeout inventory individually, records privacy-derived evidence digests, identifies the three required remote branches and explains exclusions. Additional fresh-checkout/current-native captures are represented by their published evidence or remote job identities.

- Tracked source, manifests, `Cargo.lock`, `go.sum`, fixtures, generators and `.gitattributes` are the required reproducible inputs. Exact public Git dependency pins in `Cargo.toml` fetched successfully, including CoreKit LLM `04d1411adb57aa602b992509121011aa7666ff1a`. Do not alter a pin to hide a fetch/permission failure.
- This repository has no required Git submodule or LFS object in the inventoried inputs. The existing line-ending rules preserve byte-bound Go/oracle fixtures on Windows. There is no required local `.cargo/config.toml` override.
- Own ignored files were Cargo `target/` outputs and one Python `__pycache__` artifact. They are rebuilt, not private input. No own stash, uncommitted source or unpublished branch tip remains. Worktrees/original artifacts are retained, not deleted merely because publication succeeded.
- Old local audit/harness/GUI/fuzz artifacts and branches `fix/blocker-lane-20260930` and `handoff/20260930-cloud` are outside this session's ownership and unchanged. The older `docs/handoffs/end-work-cloud.md` remains historical; use **this dated document** for the current branch, not its obsolete branch reference.
- Raw model transcripts, private execution prompts, the large stale local coordination report, credentials, provider settings, personal memory, live application stores and private user data are excluded. Essential findings/decisions are transferred here. This is not a chat or workstation backup. Privacy derivatives redact only private path metadata; timings, counts, outcomes, source revisions and protocol evidence remain intact.

GitHub/package-registry/module-proxy network access is needed for setup. Public dependency fetch succeeded from the fresh local clone. `gh` log/check/merge reads may require a suitably authorized GitHub login; never embed a token in commands or files. Retained raw job URLs are useful originals but may expire with Actions retention, so required named-case and failure evidence is also versioned here.

A generic Linux cloud runtime cannot establish native Windows reset/handle/ACL behavior, Darwin socket options, GUI/Keychain trust, physical hardware calibration or native signal/process behavior. The current HTTP matrix was executed on its actual six native GitHub runners. Future changed code needs applicable native evidence again.

**Target cloud runtime, its credentials, permissions and live-service access: not checked.** A fresh local clone proves repository completeness and the listed scoped operations, not every possible target-cloud execution. No target-cloud agent/runtime, manual workflow dispatch or paid model was started for closeout. Publication may trigger the repository's existing PR checks; those are not target-cloud continuation proof. The previously dispatched native run was observed to completion. The own local watcher was already terminated (exit -15); its termination was not used as CI evidence.

## Next work and non-negotiable gates

1. Obtain independent review of the current request-staging and bound-test delta at code commit `43c369e681e7c9124c1f7467c7b36f59b98a721c`, using the source and actual evidence in this checkout. Earlier reviews do not certify it. The local review request was unanswered; no new review or provider change was performed. Use only an already approved model/subscription, never an unapproved paid fallback.
2. Re-read PR #1145's exact current HEAD, complete review threads and required checks before any readiness or integration decision. Documentation-only commits do not rewrite the code-run identity; confirm the diff contains no source changes before reusing its native code evidence. Preserve the explicit draft while review is unresolved.
3. After verified regular integration of the HTTP fix, update the cleanup candidate from its exact preserved remote branch onto the integrated base without discarding its cleanup changes. Run the original full native acceptance, not just the cleanup subset. Keep [#1138](https://github.com/danieljustus/symaira-eraseme/issues/1138) separate from port handoff and from the unexplained LLM fixture failure.
4. Keep #1143 diagnostic-only unless separately reviewed/integrated. Do not increase the 15-second deadline, alter provider retries or close #1142 from non-reproduction alone. Preserve absolute deadlines, strict HTTP parsing, 64 KiB capture, OS trust/ACL/process requirements and real native evidence.
5. No admin bypass, force-push, release/cutover claim, fabricated fixtures, secret export, destructive Go retirement or invented observation period. Read the tracked migration ledger, task graph, contract matrix and product boundaries before advancing unrelated migration gates; this handoff is not authorization for a new backlog/release sweep.

### Copyable continuation request

Work in `danieljustus/symaira-eraseme` on `agent/http-reset-read-1144` from the exact final remote HEAD in the publication record. Read `docs/rust-port/handoffs/2026-10-03-http-fixture-cloud.md`. Verify HEAD and setup, then independently review the request-staging/bound-control delta at code commit `43c369e681e7c9124c1f7467c7b36f59b98a721c`; preserve the draft and pending review/integration gates, all original timing/byte/native requirements and the separate cleanup/LLM branches. Target-cloud runtime remains unverified.
