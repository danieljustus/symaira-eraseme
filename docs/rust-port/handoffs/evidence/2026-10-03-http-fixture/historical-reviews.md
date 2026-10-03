# Historical independent static reviews

These verdicts bind only to the exact revisions written below. None reviews the final request-staging and bound-control change at 43c369e681e7c9124c1f7467c7b36f59b98a721c. A positive static verdict never overrides its failing native run.

## eraseme-1138-1140-independent-review-result.txt

**Candidate A — ACCEPT**
HEAD verified: `b331f604e16765ff12853b05d17ec7e7e06c5e01`. Worktree clean, including untracked files.

- No blocking code findings. `crates/symeraseme-cli/src/mcp/handler.rs:3077` clears the compiler environment and confines all four temporary variables to the owned root, preserving HOME/XDG isolation, module-cache and toolchain handling.
- At `handler.rs:3098`, the probe layers the actual compiler’s explicit environment over unowned sibling temporary settings, invokes real `go env GOTMPDIR`, and compares its result before compilation. Rust borrows/lifetimes and native paths are sound. Removing or misdirecting GOTMPDIR fails the probe; the other three overrides are verified by inspection.
- Current-checkout source binding, six-case assertions and storage controls remain intact. These compare parsed JSON results, not raw byte identity.
- **CI eligibility:** all six supplied native logs identify this exact HEAD and show the modified test passing. Native acceptance evidence supports merge eligibility.

**Candidate B — CHANGES REQUIRED for merge; cleanup code accepted**
HEAD verified: `b2d282e214279ede889c011beaa78e0732130252`. Worktree clean, including untracked files.

- No blocking findings in the four-file patch. `rust-tests/parity/oracle/mcp-stdio-mutations/generate.py:20` retries only Windows WinError 32, with a five-second deadline; other errors and persistent locks propagate.
- `tests/test_mcp_stdio_oracle_cleanup.py:90` uses real Windows handles permitting read/write sharing while denying delete sharing. Both native lock tests passed on x64 and ARM64.
- Generator, source and initialize-fixture hashes match. `.github/workflows/rust-ci.yml:318` invokes cleanup controls before the non-writing oracle check; both Windows logs confirm all 670 cases passed.
- **Merge blocker:** `crates/symeraseme-core/tests/llm_http_failure_next.rs:226` applies one 15-second accept deadline across the server’s lifetime. The x64 log records `context_overflow_response_matches_go_with_secret_redaction` failing at line 237 with error 10035, then exit 101. Correct the fixture’s timeout handling and obtain green native acceptance evidence for the resulting HEAD. Run `37045269185` is not a green full matrix.

No local builds or repository tests were run.

## eraseme-1144-1142-independent-review-result.txt

**A — ACCEPT** at `728fbcd2b48a77dda88634c52c03361d30af0ab8`.

No actionable findings. The shared nonblocking reader enforces the absolute deadline and 64 KiB cap, handles EOF/reset, retries interruption, and propagates other errors. Both callers retain completeness checks. The synchronized TCP reset control preserves bytes and rejects truncation; Apple explicitly checks the former EINVAL boundary. The unchanged dribbling control remains distinguishing. Production responses, authentication and parity assertions are unchanged.

**Pending gate:** native run `37058595903`; code acceptance does not establish runtime success or merge eligibility.

**B — ACCEPT** at `55e6d05e41d46f7237a9b899bd2e7417a745a496`.

No actionable findings. Added stderr diagnostics expose only fixed case identifiers, status, counts, elapsed time and outcome booleans—no keys, bodies or prompts. The exact 15-second accept deadline, requests, oracle assertions, error checks and retry policy remain unchanged.

May retain diagnostic evidence. **This is not a timeout repair; issue #1142 must remain open.** The reported six-target pass and timings do not explain the original Windows x64 failure. Merge eligibility requires verified native checks for this HEAD; I did not inspect run `37050932501`.

Both exact HEADs, bases, clean states and two-file scopes were independently confirmed and rechecked. Static review only: no builds, tests, CI-log inspection, provider calls or mutations performed.

## eraseme-1144-final-head-review-result.txt

**ACCEPT — static review only**, bound to HEAD `2b6913eeda1f8a5f058c53349df6a0deb78c1919`.

No actionable findings. Read the full two-file diff against `ff75d89716bc948185c7cb38e9ff43259e0081d4` and all shared-reader callers.

The OS distinction is sound: Windows retains complete FIN capture and strict truncated-body rejection; Unix adds buffered-reset controls. Apple explicitly requires native EINVAL after reset before recovering bytes. Write/close synchronization, array cfg attributes, deadline/cap paths and cleanup introduce no blocking concern. Empty and truncated responses still fail validation. The shared-reader blob matches the previously reviewed commit exactly.

HEAD and clean worktree confirmed before and after. No builds, edits or provider calls performed.

**Pending gate:** native run `37061056628` must pass for this exact HEAD. Previous-head runtime/Clippy and Darwin POSIX analogue evidence do not establish final-head acceptance; the earlier Windows failure remains valid evidence.

## eraseme-1144-diagnostic-review-result.txt

**ACCEPT — static suitability of the exact diagnostic delta only.** No actionable findings.

Reviewed both full changed functions and all callers:

- `mcp_http_headers_process.rs:123,372,380`: both exchange calls use fixed `rust`/`go` labels and case indices. Diagnostics report byte count, CRLFCRLF presence, elapsed milliseconds and thread ID before parsing.
- `support/mcp_http_port.rs:162,237`: EOF/reset labels reflect the actual read result. Traced direct regression/exchange callers and readiness callers across header, Unix and Windows process tests. Added output contains no bodies, header/token values, prompts or credentials.
- Deadlines, caps, request bytes, retries, error handling, parsing and equality assertions remain unchanged. Logging adds timing overhead but does not extend deadlines. Imports, formatting arguments and updated signatures are statically consistent; `git diff --check` passed. No builds or tests ran.

Native diagnostic run **37066468147 remains pending** per supplied context. Its exact-HEAD compilation, complete header parity and required merge checks remain gates. The parent Windows ARM64 failure is unresolved; earlier controls and separate process tests do not establish complete header parity. Diagnostic success or non-reproduction would establish neither repair nor release acceptance and cannot alone close **#1144**.

Exit verification: clean worktree; HEAD **a085707b58bac61278529a6b2cbcf329a5910cc4**, parent **2b6913eeda1f8a5f058c53349df6a0deb78c1919**. No edits, builds, pushes, config/secret inspection or other agents/providers used.
