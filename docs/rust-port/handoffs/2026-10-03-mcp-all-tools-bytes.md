# All MCP tools: native byte comparison acceptance

Issue #1124 requires complete response comparisons for all 26 catalogue tools
on six native targets. Schema/value comparisons alone do not meet this gate.
`mcp-tools-native.yml` executes these case sets through Rust production dispatch,
with recorded or freshly captured responses from the real Go handler/server.

| Tools | Complete response evidence selected by the workflow |
|---|---|
| `redact_file`, `validate`, `grant`, `generate_scheduler` | Eight retained basic-tool cases, including missing-file and catalogue errors. |
| `plan_show`, `list_requests`, `get_events` | Twenty retained empty/seeded-store cases; fresh corrupt-store errors. |
| `manual_tasks_list`, `manual_tasks_show`, `manual_tasks_complete`, `manual_tasks_cleanup` | Eight retained manual-task cases; fresh store errors for list/show/complete; cleanup with a corrupt database proves its filesystem-only scope. |
| `poll_inbox` | Nine retained scripted-mailbox responses, including transport failure and required/type errors. |
| `plan_create`, `execute` | Five real stdio cases with private profiles, store and consent; dry/live denial and local manual fallback. |
| `schedule_install`, `schedule_uninstall`, `schedule_status` | Fresh unsupported-platform responses on every native target; retained private-crontab/install responses additionally execute on Unix. Unsupported platform is a real handler error before any host scheduler command. |
| `classify_reply`, `generate_rebuttal` | Eight real Go/Rust native stdio responses, with a compiled synthetic agent, saved/no-save effects, missing reply, invalid input and empty-message fallback. The accompanying 16 CLI inputs also execute on every platform. |
| `get_calendar`, `get_dashboard_data` | Six freshly captured complete responses over fixed empty/populated stores, horizon and campaign boundaries; fresh store failures. |
| `list_brokers` | Entire registry (including inactive/disabled), DE filter, empty filter and invalid boolean. |
| `generate_dashboard`, `generate_report` | Complete fixed-clock HTML responses, invalid report format and corrupt-store errors. |
| `run_web_form` | Preview and missing broker; existing accepted manual fallback (#809) is preserved. |
| `auto_confirm` | Four fresh stored-reply frames (preview/manual/no-link), exact persisted effects; fresh missing reply and invalid request ID. |

Local evidence: the original nine remaining-tool cases failed at the broker
field order, then integral form-spec float spelling. Both fixes passed all
nine. Expanding to ten actual corrupt-database failures exposed Rust's extra
SQLite prefix, which changed MCP sanitization for SQLite error code 26. Store
open now preserves Go's `eventstore: ping: file is not a database (26)` for
that structured error code only; all other storage/panic marker handling
remains source-bound. All 25 expanded cases, including the complete embedded
registry, pass locally with nine verified source hashes. The four upgraded
stored-reply auto-confirm frames also pass locally.

Six calendar/dashboard comparisons formerly checked only parsed values; they
now additionally capture Go's production MCP envelope and compare full bytes.
Reporting uses the existing Go handler's optional clock for HTML generation;
its default remains `time.Now`. Current-source commit
`0d34c46348932b076086730f2adf72a23bef533a` changes only those three clock calls.
Actual Go recaptures of scheduler, campaign, CLI-triage and agent-error fixtures
changed only source metadata, with requests, responses and effects unchanged.
Historical failed comparisons remain in the local logs. Native run
`37137670713` passed all six targets at implementation source
`0980de4edd354b69f77ea5c86a0001ecc6b2d16f`, checked out as clean PR merge
`b45637860246dc51ca02a5158326b14bfbf0af01` against main
`37cdc851efb39eb72e22653fdc724f563d505e64`. MCP-003/004/005 now pass their
scoped response-byte contracts. Final PR checks and main integration remain
required; aggregate task 8.1 still includes #1126's HTTP acceptance and task
8.5 remains gated by all handler/CLI predecessors.

| Native target | Successful job |
|---|---|
| Linux amd64 | 111245403116 |
| Linux arm64 | 111245403136 |
| Windows amd64 | 111245403235 |
| Windows arm64 | 111245403137 |
| macOS arm64 | 111245402961 |
| macOS amd64 | 111245403884 |

All six logs show 97 selected production MCP tests, including the 25 complete
current-Go responses and verified nine source hashes. Native stdio executes
ten tests on Unix and six on Windows; the difference is platform selection,
not skipped execution counted as evidence. The two native-agent tests execute
all eight raw MCP comparisons, all 16 CLI inputs and ten agent controls on
every target. Local exact-source workspace verification passed 591 tests with
two intentional ignores and strict all-target/all-feature Clippy. Full native
workspace run `37137772972` still has an Intel Mac pending at acceptance time;
its completed targets do not substitute for that final check.

Go runtime roots clear the environment and isolate HOME/USERPROFILE/XDG/temp
and database paths. The large public registry capture has an explicit 8 MiB
bound; runtime remains 30 seconds, build 180 seconds, and stderr 64 KiB. The
compiled native triage fixture is shared with #1120; it does not establish the
independent OS trust gate #1119. The staged Windows HTTP request repair is
retained from #1126 to avoid the demonstrated partial-request rejection race.

The first full native workspace run at `47d5845` exposed two additional
source-bound cancellation fixtures: `agent-cancel/http.json` and
`provider-cancel/http.json` still named the previous Go handler hash.
Actual private-root Go 1.26.6 recaptures changed only that hash, to
`7cb51f367fe003621f691ae5dcdab731a88a01fa99ba6dc6403ddaff89edeef8`.
Handler/client cancellation, provider cancellation, disconnect and child-exit
observations remain byte-identical. Failed full native jobs
111222283345/111222283270/111222283296 and PR jobs
111222281063/111222281307 retain the original failure; targeted MCP-tool
checks passed on Linux and Windows but do not substitute for the full gate.
