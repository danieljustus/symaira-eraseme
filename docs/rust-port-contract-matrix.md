# Go → Rust executable contract matrix

> Performance/release baseline: commit
> `240bf67cefa05e643e32611a02e6e7ed87a033ea` (`v0.12.1`). The corrected Go
> executable contract oracle is pinned to
> `bf53346eec234929bedf0314b99e3da85dbb991b`, after #795–#800 and #816–#817
> were fixed and independently merged. The corrected
> encryption sub-oracle is the Python↔Go conformance evidence from issue #798;
> Rust interoperability remains gated to Phase 4 and the CRY rows below. Rust
> may not replace the default binary until every
> required row is `PASS`. `TODO` means the contract is known but its
> differential case has not yet been implemented.

## Phase 1 execution evidence

Tasks `1.1`–`1.4` are implemented and reviewed on branch
`agent/issue-803-phase1-foundation` at
`dabb907b56da8d90d743de013e31628f9278bc0f`.

| Task | Evidence | Status |
|---|---|---|
| 1.1 | Pinned Rust 1.98 workspace, three crates, Cargo.lock, shadow binary | PASS |
| 1.2 | Neutral Go↔Rust harness with process/filesystem/SQLite/HTTP/MCP comparisons | PASS |
| 1.3 | Dual-language Make targets with isolated outputs and shell-safe overrides | PASS |
| 1.4 | SHA-pinned Rust PR/native CI, audit/deny/coverage handling, Cargo Dependabot | PASS |

All local Go/Rust/Parity gates and the independent specification and
quality/security reviews passed. PR #831 merged the phase as `24126f0` and
issue #803 is closed.

## Phase 2 execution evidence

Tasks `2.1`–`2.4` are implemented and independently reviewed on branch
`agent/issue-804-phase2-foundations`; implementation head
`095eb5051ae90f209855f07e3c197326a1cab6f3` passes the local Go, Rust, parity,
Cargo-deny and Windows compile gates.

| Task | Evidence | Status |
|---|---|---|
| 2.1 | Deterministic version/build metadata and exact Go CLI bytes | PASS |
| 2.2 | Time and confirmation pure functions with committed Go differential oracle | PASS |
| 2.3 | Configuration precedence, native paths and hardened executable Go oracle | PASS |
| 2.4 | 51-node Clap tree; 120 exact CLI cases; 45 deferred CLI cases fail closed | PASS |

The Go production route remains unchanged until the later cutover phase.

## Phase 3 execution evidence

Tasks `3.1` and `3.2` were implemented and independently reviewed at
`b3b7ce642944c4fd0c123150e25d5d990f043b05` and
`4a64892d5eaa0c5f435d4ceb2ce42e28a8ec449c`. The schema-v1 models, strict
validation, embedded 1,277-broker corpus, filters and validated atomic sync
pass locally in Go and Rust. Both specification reviews are `PASS`; both
quality/security reviews are `APPROVED`. Task `3.3` is independently approved
at `9fb768805de213972db6f5437f1dd218e09e88d9`; all 11 canonical templates
match the external golden fixture byte-for-byte with bounded rendering.
Task `3.4`
is independently approved at `0cfa8c0a5dccd0b189ad2cbbc076f53eab38ded2`;
the shared redaction corpus, profile-aware byte behavior, bounded review and
capability-safe file reads pass locally. Phase 3 was integrated by #870 as
`fa366dca`; its Go reference remains pinned and executable. Phase 3 is
integrated, but its later cutover rows remain governed by the matrix below.

Comparison modes: **byte** = raw byte equality; **semantic** = parsed equality
with only documented normalization; **side-effect** = status plus filesystem,
SQLite, network transcript or process behavior.

| ID | Seam | Contract / expected output or side effect | Go oracle / input fixture | Planned Rust/parity test | Mode | Platforms | Status |
|---|---|---|---|---|---|---|---|
| BASE-001 | baseline | Go format/test/lint/vet/build | `make fmt-check test lint vet build` | pre-flight script | side-effect | macOS/Linux | PASS |
| BASE-002 | baseline | exact coverage gate | `make coverage` | retain Go gate until retirement | semantic | Linux | PASS (76.23%) |
| BASE-003 | baseline | binary size/startup/RSS and release asset manifest | `v0.12.1`; `scripts/capture-go-baseline.sh` | `rust-tests/parity/baselines/v0.12.1.json` | semantic | macOS arm64 | PASS |
| CLI-001 | CLI | root help and command ordering | `symeraseme --help` | `rust-tests/parity/cases/cli/behavior.json` `help-root` (plus the 51-case `help` category) | byte | all | PASS |
| CLI-002 | CLI | root `--version` | `symeraseme --version` | `rust-tests/parity/cases/cli/behavior.json` `root-version` (`root_version` category) | byte | all | PASS |
| CLI-003 | CLI | `version` text | `symeraseme version` | `rust-tests/parity/cases/cli/behavior.json` `version` (exact group) | byte | all | PASS |
| CLI-004 | CLI | `version --json` schema v1 | `symeraseme version --json` | `rust-tests/parity/cases/cli/behavior.json` `version-json` (exact group; `schema_version` asserted) | byte | all | PASS |
| CLI-005 | CLI | global `--output text|json` inheritance | command corpus | `cli_output_modes.json` | byte | all | PASS |
| CLI-006 | CLI | unknown command/flag, usage and exit code | command corpus | `rust-tests/parity/cases/cli/behavior.json` `unknown_flag` (51), `unknown_command`, `missing_argument` (3) categories | byte | all | PASS |
| CLI-007 | CLI | shell completion: bash/zsh/fish/powershell | `completion` commands | completion snapshots | byte | all | PASS |
| CLI-008 | CLI | hidden deprecated `serve` alias and stderr notice | `serve --stdio` | alias fixture | byte | all | PASS |
| CLI-009 | CLI | `config show` text/JSON | isolated config trees | config CLI cases | byte | all | PASS |
| CLI-010 | CLI | `plan create/show/execute` flags/defaults | generated argv corpus | plan CLI cases | byte+side-effect | all | TODO |
| CLI-011 | CLI | `brokers list/show` filters/defaults | embedded registry | broker CLI cases | byte | all | PASS (merged as `387a81d1`/#954; byte-exact against the Go-measured `rust-tests/parity/cases/cli/behavior.json` cases `operate-brokers-list`/`operate-brokers-show` in `crates/symeraseme-cli/tests/command_surface.rs::brokers_json_operations_match_source_bound_goldens`, plus the text filters, the `1273`/`138` counts and the `broker %q not found`/argument-count errors in `brokers_operations_match_go_text_filters_and_errors`) |
| CLI-012 | CLI | `registry list/validate/sync` | embedded/temp registry | registry CLI cases | byte+side-effect | all | TODO |
| CLI-013 | CLI | `tick` and `status` | golden DB | `cli-tick-status/cases.json` (8 measured cases over frozen rows both sides apply) | byte+SQLite | all | PASS (both commands leave the deferred stub: `plan status` prints Go's `%v` totals map (`Total: map[open:2 requests:3 resolved:1]`) and the full JSON report, `plan tick` prints `tick complete: N action(s)` and `{"actions":null,"dry_run":…,"success":true}`, and an invalid `--output` fails closed with Go's exact text; the one normalized value is `as_of` — a wall clock Go stamps with `time.Now()` and the CLI offers no injection point, so its RFC 3339 format is asserted instead of its value. Go's `plan tick` never applies separately: `--dry-run` only decides whether `run_tick` mutates and the batch limit stays disabled. `plan status` counts campaigns first, so the fixture seeds a campaign row) |
| CLI-014 | CLI | `schedule install/uninstall/status` | isolated HOME + empty PATH | `cli-schedule/cases.json` (13 measured cases: dry-run text/JSON, unsupported platform for all three, launchd refusing a Python-era unit and replacing it once `--replace-legacy` is passed, install failing without `launchctl`, status text/JSON, status with an invalid `--output`, uninstall text/JSON) | byte+filesystem | native OS | PASS (all three commands leave the deferred stub: each is byte-exact against the committed Go capture, including stdout, stderr, exit code and every file the run leaves behind, hashed after folding only the volatile install root and the CLI's own resolved executable path. PATH is empty in both captures, so the answers are the CLI's own and no real `launchctl`/`systemctl`/`crontab` is touched. Two Go behaviours that this slice had pinned as measured were subsequently **fixed as a contract change** (#1000): `Status` now resolves launchd units under the label, the same name `Install`/`Uninstall`/the legacy scan use, so status no longer reports a removed unit as installed; and the generated units now carry the `generate-scheduler (Go)` marker, so the legacy scan no longer classifies this implementation's own output as a Python-era replacement candidate and a second `install` succeeds. Both defects were reproduced from the real Python-era generator, which emitted byte-identical launchd/systemd templates under the same file names — file names alone cannot separate the two eras, which is why the marker is the discriminator. The capture now covers 20 engine cases (including a foreign unit that still requires explicit replacement, and an install-then-uninstall-then-status case) and both entry points are pinned against it. `schedule status` still prints `success` in text form — Go discards the status payload unless JSON was requested. The legacy-replacement route, which `InstallOptions.ReplaceLegacy` gates and which no production surface could reach, is now threaded through the MCP schema, the contract handler and a `--replace-legacy` flag, and pinned by the case above (#1003); the CLI and MCP schema change also moved the frozen oracle corpus, which is re-frozen from this change's `main` commit rather than hand-edited) |
| CLI-015 | CLI | profile init/show | fixed RNG/key fixture | profile CLI cases | semantic+filesystem | all | TODO |
| CLI-016 | CLI | `render-template` | golden templates | template CLI cases | byte | all | TODO |
| CLI-017 | CLI | `grant` issue/list/revoke/revoke-all/dry-run | fixed clock/RNG | consent CLI cases | byte+filesystem | all | TODO |
| CLI-018 | CLI | dashboard/calendar/requests/events JSON shapes | golden DB | read-model CLI cases | byte | all | TODO |
| CLI-019 | CLI | reports/dashboard files | golden DB | report CLI cases | byte+filesystem | all | TODO |
| CLI-020 | CLI | manual task list/show/complete/cleanup | golden DB/temp files | manual-task CLI cases | byte+side-effect | all | TODO |
| CLI-021 | CLI | review/redaction positional and flag aliases | redaction fixtures | redaction CLI cases | byte | all | TODO |
| CLI-022 | CLI | inbox/classify/rebuttal argument aliases/defaults | mock adapters | reply CLI cases | byte+transcript | all | TODO |
| CLI-023 | CLI | web form/auto-confirm dry-run and fallback | fake driver | web CLI cases | byte+side-effect | all | TODO |
| CLI-024 | CLI | migrate inspect/run/verify/rollback/resume | migration fixtures | migration CLI cases | byte+filesystem | all | TODO |
| CFG-001 | config | defaults | no config/env | unit + differential | semantic | all | PASS |
| CFG-002 | config | global TOML path | isolated HOME/XDG | unit + differential | semantic | all | PASS |
| CFG-003 | config | project `.symeraseme.toml` | isolated cwd | unit + differential | semantic | all | PASS |
| CFG-004 | config | defaults→global→project→env precedence | conflict fixture | unit + differential | semantic | all | PASS |
| CFG-005 | config | `SYMERASEME_DATA_DIR/DB_DIR/ENCRYPT_DB/PORT/ALLOW_REMOTE` | env matrix | unit + differential | semantic | all | PASS |
| CFG-006 | config | missing/malformed/unknown TOML values | config corpus | negative cases | byte+exit | all | PASS |
| REG-001 | registry | manifest/schema version agreement | committed registry | registry conformance | semantic | all | PASS (local; native CI pending) |
| REG-002 | registry | all 1,277 embedded brokers load | `registry validate` | full-corpus test | semantic | all | PASS (local; native CI pending) |
| REG-003 | registry | four golden + one invalid fixture | `tests/fixtures/registry-contract` | shared fixtures | semantic | all | PASS (local; native CI pending) |
| REG-004 | registry | strict unknown fields and channel `oneOf` | negative corpus | property/unit tests | semantic | all | PASS (local; native CI pending) |
| REG-005 | registry | defaults, enums and optional verification | fixture corpus | model tests | semantic | all | PASS (local; native CI pending) |
| REG-006 | registry | skip `_` docs, filename=id, deterministic ID sort | temp registry | loader tests | semantic | all | PASS (local; native CI pending) |
| REG-007 | registry | filters/status/include-disabled/inactive | full corpus | filter snapshots | byte | all | PASS (local; native CI pending) |
| REG-008 | registry | HTTPS sync, validation and atomic replacement | mock server/temp dir | transcript+manifest | side-effect | all | PASS (local; native CI pending) |
| TMP-001 | templates | all 11 legal templates | `golden-templates.json` | shared golden test | byte | all | PASS (local; native CI pending) |
| TMP-002 | templates | missing/invalid variables and template names | negative corpus | error snapshots | byte | all | PASS (local; native CI pending) |
| RED-001 | redaction | PII regex and literal-profile replacement | package fixtures | shared text corpus | byte | all | PASS (local; native CI pending) |
| RED-002 | redaction | file review/interactive consent and safe paths | temp files | side-effect cases | byte+filesystem | all | PASS (local; native CI pending) |
| DB-000 | SQLite | production honors persistent default, DB_DIR and ENCRYPT_DB | isolated reproduction; issue #796 | fixed Go oracle test | side-effect | all | PASS |
| DB-001 | SQLite | schema v2/table/index SQL and `user_version = 2` | fresh Go DB | schema dump comparator | byte/semantic | all | PASS (local; native CI pending) |
| DB-002 | SQLite | WAL, busy_timeout, foreign_keys | fresh connection | PRAGMA snapshot | semantic | all | PASS (local; native CI pending) |
| DB-003 | SQLite | read existing `golden-campaign.db` | committed fixture | Rust open/query test | semantic | all | PASS (local; native CI pending) |
| DB-004 | SQLite | projection fold and `(occurred_at,id)` order | `golden-projection.json` | shared golden test | byte | all | PASS (local; native CI pending) |
| DB-005 | SQLite | reports/plans/tick snapshots | four event-store JSON fixtures | shared golden tests | byte | all | TODO |
| DB-006 | SQLite | NULL and three timestamp layouts | edge-case DB corpus | query/projection cases | semantic | all | PASS (local; native CI pending) |
| DB-007 | SQLite | invalid event append vs unknown replay skip | corrupt/forward fixtures | negative cases | side-effect | all | PASS (issue #889) |
| DB-008 | SQLite | append+projection atomicity and rollback | forced failures | transaction tests | side-effect | all | PASS (local; native CI pending) |
| DB-009 | SQLite | lock/busy/concurrent readers+writes | process harness | contention tests | side-effect | native OS | PASS (local; native CI pending) |
| DB-010 | SQLite | interrupted initialization/migration/read-only DB | fault fixtures | recovery tests | side-effect | native OS | PASS (local; native CI pending) |
| CRY-000 | crypto | exact V1/V2/V3 raw headers are each 17 bytes | `internal/eventstore/encrypt.go`; issue #795 | `TestEncryptionHeaderContract` | byte | all | PASS |
| CRY-000B | crypto | Python standard-Fernet and Go format collision is resolved with interoperable, distinct versioning | `python-final` + Go; issue #798 | Python/Go vectors complete; Rust vectors remain Phase 4 gate | byte | all | PASS (Python↔Go); Rust gated |
| CRY-001 | crypto | Python-final standard-Fernet V1 decrypt | `tests/fixtures/event-store/crypto/golden-campaign-v1-python.db`, generated through `python-final` by `scripts/generate-crypto-fixtures.py` | `crates/symeraseme-core/tests/encryption_parity.rs::python_final_v1_fixture_matches_shared_go_plaintext` | byte | all | PASS (local; native CI pending) |
| CRY-002 | crypto | Python-final standard-Fernet V2 decrypt | `tests/fixtures/event-store/crypto/golden-campaign-v2-python.db`, generated through `python-final` by `scripts/generate-crypto-fixtures.py` | `crates/symeraseme-core/tests/encryption_parity.rs::python_final_v2_fixture_matches_shared_go_plaintext` | byte | all | PASS (local; native CI pending) |
| CRY-003 | crypto | Python-final standard-Fernet V3 decrypt | `tests/fixtures/event-store/crypto/golden-campaign-v3-python.db`, generated through `python-final` by `scripts/generate-crypto-fixtures.py` | `crates/symeraseme-core/tests/encryption_parity.rs::python_final_v3_fixture_matches_shared_go_plaintext` plus fixture-backed negative cases | byte | all | PASS (local; native CI pending) |
| CRY-004 | crypto | corrected Go write format decryptable by Rust | test-only Go V3 binary oracle using fresh CSPRNG material | `crates/symeraseme-core/tests/encryption_parity.rs::rust_writer_is_consumed_by_go_oracle_and_go_writer_by_rust` (Go writer → Rust reader) | byte | all | PASS (local; native CI pending) |
| CRY-005 | crypto | Rust write format decryptable by corrected Go | Rust V3 writer using OS CSPRNG salt and IV | `crates/symeraseme-core/tests/encryption_parity.rs::rust_writer_is_consumed_by_go_oracle_and_go_writer_by_rust` (Rust writer → Go reader) | byte | all | PASS (local; native CI pending) |
| CRY-006 | crypto | standard-Fernet and any distinctly-versioned Go compatibility parser reject truncation/tamper/wrong keys | mutation corpus | `crates/symeraseme-core/tests/encryption_parity.rs` + `fuzz/fuzz_targets/encryption_envelope.rs` | semantic | all | PASS (local; native CI pending). 27 pinned cases — the nine mutations (`valid`, `malformed_header`, `short`, `short_token`, `wrong_key`, `hmac_tamper`, `gcm_failure`, `raw_standard_valid`, `raw_standard_tamper`) × V1/V2/V3 — replay in `cry006_table_consumes_every_pinned_go_oracle_case` against `tests/fixtures/event-store/crypto/cry006-go-oracle.json`, generated by `rust-tests/parity/oracle/cry006/main.go`: the oracle commit, the oracle source digest, the generator digest and every fixture digest are pinned, and `cry006_source_provenance_rejects_mutated_digest` proves that digest check is not vacuous |
| CRY-007 | crypto | decrypted temp dir/file modes and cleanup | isolated TMPDIR | filesystem manifest | side-effect | native OS | PASS (local; native CI pending). `crates/symeraseme-core/tests/storage_encrypted_lifecycle.rs` pins the decrypted temp file at `0600` inside a `0700` temp directory and proves file, lock and directory entry are removed on close (`encrypted_open_uses_canonical_path_and_private_sqlite_temp`), the cleanup after a failed open (`failed_store_open_removes_temp_lock_after_temp_deletion`), that a plain open does not chmod an existing canonical parent (`plain_open_does_not_chmod_an_existing_canonical_parent`) and four scavenger rules (`scavenge_preserves_registered_active_temp_even_when_old`, `scavenge_preserves_stale_sidecars_when_main_temp_is_locked`, `scavenge_retries_main_after_sidecar_cleanup_failure`, `scavenge_does_not_create_lock_for_recent_unregistered_temp`, `scavenge_preserves_unregistered_decrypted_temp_for_other_process`); mode assertions are `cfg(unix)`-only because the fixture is minted on macOS and replayed in the 3-OS matrix |
| CRY-008 | crypto | WAL checkpoint before re-encryption | write/close/crash corpus | data durability test | side-effect | all | PASS (local; native CI pending). `close_store_inner` (`crates/symeraseme-core/src/storage/encrypted_store.rs`) checkpoints the WAL before re-encrypting the temporary database; `storage_encrypted_lifecycle.rs::finalise_all_reports_an_active_store_and_close_keeps_wal_data` writes a WAL-resident row, closes the encrypted store and reads that row back from the reopened store — the assertion fails if the checkpoint is skipped. Crash recovery of an interrupted initialisation/migration stays with DB-010 |
| ID-001 | identity | encrypted profile Go→Rust→Go | deterministic vector | bidirectional harness | byte/semantic | all | TODO |
| ID-000 | identity | Python/Go profile path, serialized fields, hash bytes, and decrypt-only key lookup are frozen | Python fixture; issue #816 | identity interoperability/regression tests | byte/side-effect | all | PASS |
| ID-002 | identity | master-key resolution order and aliases | fake env/keyring/symvault | adapter tests | semantic | native OS | TODO |
| ID-003 | identity | no secrets in errors/logs | sentinel secrets | `crates/symeraseme-core/tests/identity_secret_resolution.rs` | byte | all | PASS (local; native CI pending) |
| ID-004 | consent | token filename/hash/content/expiry/command | fixed clock/RNG | `crates/symeraseme-core/tests/consent_api.rs` | byte | all | PASS (focused Rust contract; task 4.7 remains open) |
| ID-005 | consent | 0700 dirs, 0600 files, atomic updates | isolated HOME | filesystem manifest | side-effect | native OS | TODO |
| DOM-000A | domain | production `poll_inbox` uses a real adapter and persistent HWM | fake-server transcript; issue #799 | corrected Go oracle | side-effect | all | PASS |
| DOM-000B | domain | production web form has an honest tested runtime/manual boundary | local executor contract + durable manual-task tests; issue #800 | `TestWebFormNoExecutorPersistsManualFallback`, `TestWebFormExecutorReceivesBoundedContextAndMapsEvidence`, `TestAutoConfirmCreatesManualConfirmationTaskWithoutClick` | side-effect | all | PASS |
| DOM-001 | domain | deadlines/tick transitions: policy, due-request scan and applying actions | `golden-tick.json` (Go `TestGoldenTickConformance`, `TestApplyTickActionsWritesEvents`) | `crates/symeraseme-core/src/deadlines.rs` (`actions_for_candidate`, `run_tick`, `apply_tick_actions`), shared golden + apply tests | byte | all | PARTIAL (policy matches the golden byte-exactly, `run_tick` scans due candidates, `apply_tick_actions` appends with source `scheduler`, dry runs write nothing and projections are rebuilt; the CLI `tick`/`status` adapters are CLI-013, ported byte-exactly against frozen rows) |
| DOM-002 | domain | campaign plan (`PlanCampaign`) plans the same requests and `PLANNED` payloads as Python; execution transitions remain open | `golden-plan.json` (Go `TestGoldenPlanConformance`) | `crates/symeraseme-core/src/campaign.rs`, shared golden test | byte+SQLite | all | PARTIAL (planning, registry matching, channel/template/jurisdiction resolution and the event payloads pinned byte-exactly against the golden; `GetPlan`/`ExecuteCampaign` and the execution transitions still pending) |
| DOM-003 | domain | reporting/dashboard aggregation (`GetReportData`, `GetDashboardData`, `GetCampaignStatus`, `GetCalendar`) | `golden-reporting.json` (Go `TestGoldenReportingConformance`) | `crates/symeraseme-core/src/reporting.rs`, shared golden test | byte | all | PARTIAL (all four sections match the shared golden including Go's own anchor values; the HTML/CSV/JSON export renderers `GenerateReport`/`GenerateDashboard` still pending. Equal-total ordering is pinned on both sides by `internal/reporting/order_regression_test.go` and `tied_totals_order_by_broker_id`; the golden itself has no tie, so it cannot cover that rule.) |
| DOM-004 | domain | reply classification/rebuttal mapping | broker reply fixtures | shared corpus | byte/semantic | all | PARTIAL. Pinned: the pure mapping — classification parsing (clamps, defaults, `extracted_fields`), rejection classification (`key_points` null/empty/all-non-string/mixed), the ordered wire fields and the deterministic fallback template — in `crates/symeraseme-core/tests/triage_contract.rs` against `tests/fixtures/triage-contract/dom004a.json`, whose `oracle_revision` and per-source `source_sha256` (classifier.go, rebuttal.go) are asserted; plus prompt construction byte-for-byte in `triage_prompts.rs`/`dom004b`. Open: the LLM-backed service paths (`replies.Service.ClassifyReply`/`GenerateRebuttal`, `triage.ReplyClassifier`) and the shared broker-reply corpus. `tests/fixtures/broker_replies/` survives from the Python implementation, is consumed by no Go or Rust test today and is therefore not evidence |
| DOM-005 | domain | MIME bytes, headers, boundary, CRLF, recipients | fixed time/message ID | raw SMTP fixture | byte | all | TODO (the header subset — folding, RFC 2047 words, thread/Message-ID extraction, date fallback — is pinned by the DOM-006 oracle; MIME boundaries, CRLF and recipients stay open) |
| DOM-006 | domain | IMAP UIDVALIDITY/HWM/search/fetch policy | fake transcript | state-machine tests | side-effect | all | PARTIAL (plain-TCP transport ported: nine transcript cases replay byte-exactly in `crates/symeraseme-core/tests/imap_transport_parity.rs` — CAPABILITY, quoted/raw mailbox names, LOGIN and XOAUTH2 SASL-IR, read-only EXAMINE, UID SEARCH with CHARSET, collapsed UID FETCH with literals, both bounded sections, the cleartext refusal, the cleanup LOGOUT after a failed login and the exact error texts; The plain path is NOT production-reachable: `AllowInsecureCleartextAuth` is never set by any product path (types.go:68 documents it as an explicit test escape hatch), so a real mailbox is always reached over implicit TLS or STARTTLS — TLS is therefore the next required piece, and `poll_inbox` cannot be wired before it. Adapter-agnostic policy ported: persisted-HWM UID range, UIDVALIDITY cold start, exhausted UID space, oldest-pending-window truncation, FETCH-omission refusal, since window, cross-folder Message-ID dedupe, staged HWM commit and its failure path, credential redaction in raw and all four base64 spellings, and the `IMAP_*` config contract — byte-exact against `rust-tests/parity/oracle/email` in `crates/symeraseme-core/tests/email_parity.rs`; TODO: the network dialer and SASL transport, Go still owns `dialer.go`/`oauth2.go`, tracked by DOM-007) |
| DOM-007 | domain | OAuth2 state, PKCE, refresh and redaction | mock HTTP server | transcript/files | side-effect | all | PARTIAL (ported: provider table, authorize URL with S256 PKCE, one-time state store with consumption/expiry/atomic file and permissions, token exchange and refresh with status-only errors; byte-exact against `rust-tests/parity/oracle/oauth2` in `crates/symeraseme-core/tests/oauth2_parity.rs`, whose token cases record the transcript of a real loopback HTTP request; TODO: `RedactError` and the SASL XOAUTH2 use of the token — both live in `dialer.go`, DOM-006's transport remainder) |
| DOM-008 | domain | LLM provider descriptors, retries/errors | mock HTTP corpus | `llm-provider-surface/cases.json` (8 resolution cases, 9 retry cases, 3 usage records, 7 hash inputs, all measured on the real Go implementation over frozen inputs) | side-effect | all | PARTIAL (pinned byte/type-exactly: the provider name set, the resolution order and its normalization, the unknown-provider error text, the usage record's six-key map, the error taxonomy's display and unwrap shape, the retry loop's attempt accounting and backoff arithmetic, the FNV-1a cache-key jitter with the empty-key short-circuit, and Go's `%!w(<nil>)` exhaustion text from the zero-value client. Still Go: the `anthropic`/`openai`/`ollama`/`openai-compatible` transports, their credential reference resolution and their `auth_failure` texts — all corekit/llmkit, with no Rust counterpart; the fixture records the measured text of that path as a boundary. Also still Go: the host-agent subprocess protocol (CLI detection is ported and pinned, but the invocation, timeout and exit-code wrapping need a fake CLI and are not covered here) |
| DOM-009 | domain | scheduler bytes/paths/install commands | isolated HOME + fake exec | native snapshots | byte+side-effect | macOS/Linux/Windows | PARTIAL (PASS: `Generate`/`WriteFiles` byte-exact against `rust-tests/parity/oracle/scheduler`, both directions reviewed for a path-traversal regression and fixed, #907; legacy-Python unit detection (`DetectLegacyPythonUnit`/`DetectLegacyPythonUnits`/`ScanLegacyPythonUnits`) byte-exact against the same oracle, reviewed and fixed (task 5.4 legacy-detection slice); Install/Uninstall/Status are ported byte-exactly against the isolated-HOME fake-exec oracle (task CLI-014, 20 cases; the #1000 contract defects in `Status`'s launchd path and the self-blocking re-install are fixed in both implementations and pinned by regression tests, and the Windows-specific traversal case still runs only on Windows)) |
| DOM-010 | domain | manual-task queue: create/list/get/complete, redacted evidence and artifact cleanup | Go `internal/manualtasks` tests | `crates/symeraseme-core/src/manualtasks.rs`, filesystem+DB tests | side-effect | all | PARTIAL (reason normalisation, redacted 0600 snapshots, the `HUMAN_ACTION_REQUIRED` event and its `AWAITING_USER_ACTION` rebuild, list filters, the `NOTE_ADDED` event and the cleanup counts all pinned; `service.go` orchestration stays open) |
| MCP-001 | MCP | `initialize` protocol version/capabilities/serverInfo | raw frame; `tests/fixtures/mcp-contract/initialize_cases.json` (78 measured cases) | `mcp/protocol.rs`, shared golden test | byte | all | PASS (merged; byte-exact against the Go stdio oracle) |
| MCP-000 | MCP HTTP | bearer secret uses constant-time comparison and strict header parsing | auth corpus; issue #817 | `TestServeHTTPBearerAuthContract` | side-effect | all | PASS |
| MCP-002 | MCP | exact 26-tool `tools/list` | `tools.list.json`; `mcp-002/` fixtures | `mcp/tools_list.rs`, shared golden test | byte | all | PASS (merged; 26 tools byte-exact, notifications silent, `list_tools` alias accepted) |
| MCP-003 | MCP | `tools/call` dispatch through a real handler: 15 of the 26 catalogue tools execute, the rest report their state explicitly | Go `ContractHandler()`; `tests/fixtures/mcp-contract/mcp-003/cases.json` (8 measured cases), `mcp-003-store/{empty,seeded}-cases.json` (20 measured cases over `seed.sql`) and `mcp-003-poll/cases.json` (9 measured cases over `seed.sql` + `mailbox.json`) | `crates/symeraseme-cli/src/mcp/{tools_call,handler}.rs`, fixture loop plus shape tests | byte/semantic | all | PARTIAL (byte-exact against Go's real handler: `redact_file`, `validate`, `manual_tasks_list/show/complete/cleanup`, `grant` dry run, `generate_scheduler` dry run, plus `plan_show`, `list_requests`, `get_events` over frozen rows — those three read the store, whose Go write path stamps `created_at`/`recorded_at` with `time.Now()`, so the rows are fixture input both sides apply — and `poll_inbox` over a scripted mailbox: Go's oracle drives the real handler with an injected `IMAPDialer`/`HWMStore`, Rust replays the same `mailbox.json` through the same stdio entry, covering thread and subject matches, an unmatched message, multi-folder aggregation, the empty mailbox, the since-window drop, a transport failure (`-32603`) and the catalogue's argument gate (`missing required argument: port`, `invalid parameter type: folders`). That slice also closed a real defect: `parse.rs` copied an empty flag list where Go's `append([]string(nil), …)` leaves it nil, so the payload said `[]` instead of `null`. The injected transport is a seam on both sides — the real TLS/STARTTLS dialer stays Go, see DOM-007. Shape-asserted against measured Go values: `plan_create`, `list_brokers`, `schedule_install` — those three cannot be pinned because Go fills a wall clock, the row ids of the store it just wrote, or the resolved binary path. `get_calendar`/`get_dashboard_data` call `time.Now()` in the handler; `schedule_uninstall`/`schedule_status` touch the host; `execute` awaits the email slice; `classify_reply`/`generate_rebuttal` are blocked on corekit #288. See `docs/rust-port/handoffs/2026-09-19-mcp-store-reads.md`) |
| MCP-004 | MCP | success content envelope | `mcp-result/` fixture (12 measured cases) | `mcp/envelope.rs`, fixture + unit tests | byte | all | PARTIAL (envelope shape, string/object/array/number/absent results pinned byte-exactly; the HTTP transport remains MCP-009…013) |
| MCP-005 | MCP | error codes/messages and SQLite sanitization | `mcp-envelope/`, `mcp-result/` fixtures | `mcp/envelope.rs`, `mcp/tools_call.rs` | byte | all | PARTIAL (all four storage markers, the panic marker and the pass-through case pinned; tool-specific error paths follow MCP-003) |
| MCP-006 | MCP | legacy `list_tools`, `status`, bare `redact_file` | `mcp-002/`, `mcp-003/`, `mcp-006/cases.json` (12 measured cases) | `mcp/tools_list.rs`, `mcp/tools_call.rs`, `mcp/protocol.rs` | byte | all | PASS (the `list_tools` alias, the `status` alias default and the bare `redact_file` method pinned byte-exactly; the bare method keeps its own semantics rather than aliasing `tools/call` — the path may arrive as `{"path":…}` or `["…"]`, a bad argument *and* a handler failure are both `-32602` (with the sanitized message) where `tools/call` answers `-32603`, the result is returned without the content envelope, and a notification stays silent) |
| MCP-007 | MCP | notifications and mixed/empty batch behavior | `mcp-envelope/`, `mcp-stream/` fixtures | `mcp/protocol.rs`, `mcp/stream.rs` | byte | all | PARTIAL (notifications stay silent through the shared entry and the stream, empty and array payloads answer `-32600`; HTTP-level batch arrays differ in Go itself and belong to MCP-009/011) |
| MCP-008 | MCP | ID types/null/invalid IDs and params | `initialize_cases.json`, `mcp-envelope/` fixtures | `mcp/protocol.rs`, `mcp/tools_call.rs` | byte | all | PARTIAL (id shapes, null ids, invalid ids and params variants pinned byte-exactly; a property/fuzz corpus is still pending) |
| MCP-009 | MCP HTTP | POST-only, 5 MiB limit, content type | HTTP corpus | HTTP differential | byte+status | all | TODO |
| MCP-010 | MCP HTTP | bearer auth exactness | `internal/mcp/server.go`; issue #817 | `TestServeHTTPBearerAuthContract` | byte+status | all | PASS |
| MCP-011 | MCP HTTP | Origin/loopback/`--allow-remote` policy | host/origin matrix | native network tests | side-effect | native OS | TODO |
| MCP-012 | MCP HTTP | token path/mode/rotation | isolated data dir | filesystem manifest | side-effect | native OS | TODO |
| MCP-013 | MCP HTTP | SIGINT/SIGTERM and 5s graceful shutdown | process harness | signal tests | side-effect | native OS | TODO |
| MCP-014 | MCP stdio | Consecutive JSON values separated by whitespace — **not** newline frames (measured: a value may span lines, three values may sit on one line); one response per request, notifications silent, nothing else on stdout | Go `ServeStdio`; `tests/fixtures/mcp-contract/mcp-stream/cases.json` (13 measured cases) | `crates/symeraseme-cli/src/mcp/stream.rs`, raw stream comparator | byte | all | PARTIAL (framing and response stream pinned byte-exactly; zero-stdout-pollution check and live-pipe streaming still pending) |
| MCP-015 | MCP stdio | Malformed, truncated and multiple values in one stream | same oracle fixture (adjacent, truncated, junk, scalar and string cases) | `stream.rs` error cases; fuzz corpus still pending | byte+exit | all | PARTIAL (multiple values answered, malformed/truncated input aborts the stream without a fabricated response, each pinned byte-exactly; fuzz corpus pending) |
| APP-000 | SwiftUI | `listTools()` parses raw `result.tools`, not call content envelope | exact Go response; issue #797 | `MCPClientToolsListTests` | semantic | macOS | PASS |
| APP-001 | SwiftUI | binary discovery order and name | Swift unit tests | Rust binary fixture | side-effect | macOS | TODO |
| APP-002 | SwiftUI | launch `mcp --host --port` | supervisor test | Rust E2E | side-effect | macOS | TODO |
| APP-003 | SwiftUI | token read + authenticated tools/list/call | app tests | Rust E2E | semantic | macOS | TODO |
| APP-004 | SwiftUI | app shutdown terminates backend cleanly | app/supervisor test | Rust E2E | side-effect | macOS | TODO |
| REL-001 | release | six CLI archive names | v0.12.1 release manifest | artifact manifest test | byte | native matrix | TODO |
| REL-002 | release | tar.gz/zip root contains `symeraseme`, LICENSE, README | released archives | archive inspection | side-effect | all | TODO |
| REL-003 | release | `checksums.txt` format/content | v0.12.1 | checksum verifier | byte/semantic | all | TODO |
| REL-004 | release | static/self-contained runtime expectations | released binaries | linkage inspection+smoke | side-effect | native OS | TODO |
| REL-005 | release | Homebrew URL/name/install/version test | `Formula/symeraseme.rb` | tap dry-run/audit | side-effect | macOS/Linux | TODO |
| REL-006 | release | DMG name and app bundle paths | v0.12.1 DMG | mounted DMG manifest | side-effect | macOS | TODO |
| REL-007 | release | nested Rust binary Developer ID signature | release workflow | codesign verification | side-effect | macOS | TODO |
| REL-008 | release | DMG container signature before notarization | issue #794 | codesign/notary proof | side-effect | macOS | BLOCKED #794 |
| REL-009 | release | notarization/stapling and release-note truth | release workflow | notary/stapler checks | side-effect | macOS | TODO |
| REL-010 | release | SBOM, audit, deny, provenance | new Rust workflow | artifact/security checks | side-effect | all | TODO |
| CUT-001 | cutover | explicit `SYMERASEME_BACKEND=go` fallback | dual archive | process test | side-effect | all | TODO |
| CUT-002 | cutover | Rust upgrade reads existing plain/encrypted data | copied Go user fixtures | upgrade suite | side-effect | all | TODO |
| CUT-003 | cutover | rollback Go reads post-Rust data | Rust-created copies | rollback suite | side-effect | all | TODO |
| CUT-004 | cutover | prerelease canary and stable observation period | GitHub release evidence | release checklist | side-effect | all | TODO |
| CUT-005 | retirement | Go removed only after separate approval | git/release history | retirement PR gate | side-effect | all | TODO |

## Required normalizers

No normalizer is accepted by default. Each added normalizer needs a row-specific
reason and must operate after raw capture. Expected candidates are restricted
to fixed temporary-root substitution, intentionally injected clock values,
random nonces/tokens that are also compared structurally, and platform-native
path separators where the public contract permits them.

## Matrix maintenance rule

A contract mismatch is a port defect until classified. If Go behavior is a bug
that should not survive, first create a separate contract-change issue, update
the Go behavior and fixtures, and only then port the changed contract. Never
hide a mismatch by broad JSON sorting, whitespace trimming or stderr removal.

## Ledger reconciliation 2026-09-20

Row status was re-checked against executable evidence at `c1bce67a`. No status
was raised without a named, merged test:

- `CLI-011` was already integrated by #954 (`387a81d1`) and is pinned by the
  Go-measured `rust-tests/parity/cases/cli/behavior.json`; the row still said
  `TODO` and is now `PASS`.
- `CRY-006`, `CRY-007` and `CRY-008` were implemented and pinned by
  `47bb0309`/`d19ba0bb` and the WAL/durability paths; the rows said `TODO`.
  Closing the gap honestly required one addition: the decrypted temp
  *directory* mode (`0700`) had no positive assertion, so
  `storage_encrypted_lifecycle.rs::encrypted_open_uses_canonical_path_and_private_sqlite_temp`
  now pre-creates the temp root at `0755` and asserts that `open_encrypted`
  tightens it to `0700`. Negative control: `ensure_private_dir` mutated to
  `0770` → assertion red (`504 != 448`), restored → green.
- `DOM-004` also said `TODO`; its pure mapping is pinned by `triage_contract.rs`
  (Go-oracle `dom004a.json` with per-source digests) and `triage_prompts.rs`.
  It is now `PARTIAL`, because the LLM-backed service paths and the shared
  broker-reply corpus are genuinely open — `tests/fixtures/broker_replies/`
  is Python-era residue that no current test reads.
- Rows whose evidence could not be named were left untouched rather than
  upgraded on plausibility.
