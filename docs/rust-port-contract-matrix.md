# Go → Rust executable contract matrix

> Performance/release baseline: commit
> `240bf67cefa05e643e32611a02e6e7ed87a033ea` (`v0.12.1`). The executable
> contract oracle is pinned to the merge commit after #795, #796 and
> #798–#800 and #816–#817 are fixed;
> Task 0.4 must replace this sentence with that exact commit before Rust
> behavior is implemented. Rust may not replace the default binary until every
> required row is `PASS`. `TODO` means the contract is known but its
> differential case has not yet been implemented.

Comparison modes: **byte** = raw byte equality; **semantic** = parsed equality
with only documented normalization; **side-effect** = status plus filesystem,
SQLite, network transcript or process behavior.

| ID | Seam | Contract / expected output or side effect | Go oracle / input fixture | Planned Rust/parity test | Mode | Platforms | Status |
|---|---|---|---|---|---|---|---|
| BASE-001 | baseline | Go format/test/lint/vet/build | `make fmt-check test lint vet build` | pre-flight script | side-effect | macOS/Linux | PASS |
| BASE-002 | baseline | exact coverage gate | `make coverage` | retain Go gate until retirement | semantic | Linux | PASS (76.23%) |
| BASE-003 | baseline | binary size/startup/RSS and release asset manifest | `v0.12.1`; `scripts/capture-go-baseline.sh` | `rust-tests/parity/baselines/v0.12.1.json` | semantic | macOS arm64 | PASS |
| CLI-001 | CLI | root help and command ordering | `symeraseme --help` | `cli_root_help.snap` | byte | all | TODO |
| CLI-002 | CLI | root `--version` | `symeraseme --version` | `cli_root_version.snap` | byte | all | TODO |
| CLI-003 | CLI | `version` text | `symeraseme version` | `cli_version_text.snap` | byte | all | TODO |
| CLI-004 | CLI | `version --json` schema v1 | `symeraseme version --json` | `cli_version_json.snap` | byte | all | TODO |
| CLI-005 | CLI | global `--output text|json` inheritance | command corpus | `cli_output_modes.json` | byte | all | TODO |
| CLI-006 | CLI | unknown command/flag, usage and exit code | command corpus | `cli_invalid_args.json` | byte | all | TODO |
| CLI-007 | CLI | shell completion: bash/zsh/fish/powershell | `completion` commands | completion snapshots | byte | all | TODO |
| CLI-008 | CLI | hidden deprecated `serve` alias and stderr notice | `serve --stdio` | alias fixture | byte | all | TODO |
| CLI-009 | CLI | `config show` text/JSON | isolated config trees | config CLI cases | byte | all | TODO |
| CLI-010 | CLI | `plan create/show/execute` flags/defaults | generated argv corpus | plan CLI cases | byte+side-effect | all | TODO |
| CLI-011 | CLI | `brokers list/show` filters/defaults | embedded registry | broker CLI cases | byte | all | TODO |
| CLI-012 | CLI | `registry list/validate/sync` | embedded/temp registry | registry CLI cases | byte+side-effect | all | TODO |
| CLI-013 | CLI | `tick` and `status` | golden DB | tick/status cases | byte+SQLite | all | TODO |
| CLI-014 | CLI | `schedule install/uninstall/status` | isolated HOME | scheduler CLI cases | byte+filesystem | native OS | TODO |
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
| CFG-001 | config | defaults | no config/env | unit + differential | semantic | all | TODO |
| CFG-002 | config | global TOML path | isolated HOME/XDG | unit + differential | semantic | all | TODO |
| CFG-003 | config | project `.symeraseme.toml` | isolated cwd | unit + differential | semantic | all | TODO |
| CFG-004 | config | defaults→global→project→env precedence | conflict fixture | unit + differential | semantic | all | TODO |
| CFG-005 | config | `SYMERASEME_DATA_DIR/DB_DIR/ENCRYPT_DB/RESOURCES` | env matrix | unit + differential | semantic | all | TODO |
| CFG-006 | config | missing/malformed/unknown TOML values | config corpus | negative cases | byte+exit | all | TODO |
| REG-001 | registry | manifest/schema version agreement | committed registry | registry conformance | semantic | all | TODO |
| REG-002 | registry | all 1,277 embedded brokers load | `registry validate` | full-corpus test | semantic | all | TODO |
| REG-003 | registry | four golden + one invalid fixture | `tests/fixtures/registry-contract` | shared fixtures | semantic | all | TODO |
| REG-004 | registry | strict unknown fields and channel `oneOf` | negative corpus | property/unit tests | semantic | all | TODO |
| REG-005 | registry | defaults, enums and optional verification | fixture corpus | model tests | semantic | all | TODO |
| REG-006 | registry | skip `_` docs, filename=id, deterministic ID sort | temp registry | loader tests | semantic | all | TODO |
| REG-007 | registry | filters/status/include-disabled/inactive | full corpus | filter snapshots | byte | all | TODO |
| REG-008 | registry | HTTPS sync, validation and atomic replacement | mock server/temp dir | transcript+manifest | side-effect | all | TODO |
| TMP-001 | templates | all 11 legal templates | `golden-templates.json` | shared golden test | byte | all | TODO |
| TMP-002 | templates | missing/invalid variables and template names | negative corpus | error snapshots | byte | all | TODO |
| RED-001 | redaction | PII regex and literal-profile replacement | package fixtures | shared text corpus | byte | all | TODO |
| RED-002 | redaction | file review/interactive consent and safe paths | temp files | side-effect cases | byte+filesystem | all | TODO |
| DB-000 | SQLite | production honors persistent default, DB_DIR and ENCRYPT_DB | isolated reproduction; issue #796 | fixed Go oracle test | side-effect | all | BLOCKED #796 |
| DB-001 | SQLite | schema v1/table/index SQL and `user_version` | fresh Go DB | schema dump comparator | byte/semantic | all | TODO |
| DB-002 | SQLite | WAL, busy_timeout, foreign_keys | fresh connection | PRAGMA snapshot | semantic | all | TODO |
| DB-003 | SQLite | read existing `golden-campaign.db` | committed fixture | Rust open/query test | semantic | all | TODO |
| DB-004 | SQLite | projection fold and `(occurred_at,id)` order | `golden-projection.json` | shared golden test | byte | all | TODO |
| DB-005 | SQLite | reports/plans/tick snapshots | four event-store JSON fixtures | shared golden tests | byte | all | TODO |
| DB-006 | SQLite | NULL and three timestamp layouts | edge-case DB corpus | query/projection cases | semantic | all | TODO |
| DB-007 | SQLite | invalid event append vs unknown replay skip | corrupt/forward fixtures | negative cases | side-effect | all | TODO |
| DB-008 | SQLite | append+projection atomicity and rollback | forced failures | transaction tests | side-effect | all | TODO |
| DB-009 | SQLite | lock/busy/concurrent readers+writes | process harness | contention tests | side-effect | native OS | TODO |
| DB-010 | SQLite | interrupted initialization/migration/read-only DB | fault fixtures | recovery tests | side-effect | native OS | TODO |
| CRY-000 | crypto | exact V1/V2/V3 raw headers are each 17 bytes | `internal/eventstore/encrypt.go`; issue #795 | `TestEncryptionHeaderContract` | byte | all | PASS |
| CRY-000B | crypto | Python standard-Fernet and Go format collision is resolved with interoperable, distinct versioning | `python-final` + Go; issue #798 | Python/Go/Rust bidirectional vectors | byte | all | BLOCKED #798 |
| CRY-001 | crypto | Python-final standard-Fernet V1 decrypt | Python-generated vector | Rust decrypt vector | byte | all | TODO |
| CRY-002 | crypto | Python-final standard-Fernet V2 decrypt | Python-generated vector | Rust decrypt vector | byte | all | TODO |
| CRY-003 | crypto | Python-final standard-Fernet V3 decrypt | Python-generated vector | Rust decrypt vector | byte | all | TODO |
| CRY-004 | crypto | corrected Go write format decryptable by Rust | fixed clock/RNG Go vector | bidirectional harness | byte | all | TODO |
| CRY-005 | crypto | Rust write format decryptable by corrected Go | fixed clock/RNG Rust vector | bidirectional harness | byte | all | TODO |
| CRY-006 | crypto | standard-Fernet and any distinctly-versioned Go compatibility parser reject truncation/tamper/wrong keys | mutation corpus | negative tests/fuzz seeds | semantic | all | TODO |
| CRY-007 | crypto | decrypted temp dir/file modes and cleanup | isolated TMPDIR | filesystem manifest | side-effect | native OS | TODO |
| CRY-008 | crypto | WAL checkpoint before re-encryption | write/close/crash corpus | data durability test | side-effect | all | TODO |
| ID-001 | identity | encrypted profile Go→Rust→Go | deterministic vector | bidirectional harness | byte/semantic | all | TODO |
| ID-000 | identity | Python/Go profile path, serialized fields, hash bytes, and decrypt-only key lookup are frozen | Python fixture; issue #816 | identity interoperability/regression tests | byte/side-effect | all | PASS |
| ID-002 | identity | master-key resolution order and aliases | fake env/keyring/symvault | adapter tests | semantic | native OS | TODO |
| ID-003 | identity | no secrets in errors/logs | sentinel secrets | output scanner | byte | all | TODO |
| ID-004 | consent | token filename/hash/content/expiry/command | fixed clock/RNG | shared cases | byte | all | TODO |
| ID-005 | consent | 0700 dirs, 0600 files, atomic updates | isolated HOME | filesystem manifest | side-effect | native OS | TODO |
| DOM-000A | domain | production `poll_inbox` uses a real adapter and persistent HWM | fake-server transcript; issue #799 | corrected Go oracle | side-effect | all | BLOCKED #799 |
| DOM-000B | domain | production web form has an honest tested runtime/manual boundary | fake process/driver; issue #800 | corrected Go oracle | side-effect | all | BLOCKED #800 |
| DOM-001 | domain | deadlines/tick transitions | `golden-tick.json` | shared golden test | byte | all | TODO |
| DOM-002 | domain | campaign plan and execution transitions | `golden-plan.json` | shared golden test | byte+SQLite | all | TODO |
| DOM-003 | domain | reporting/dashboard aggregation | `golden-reporting.json` | shared golden test | byte | all | TODO |
| DOM-004 | domain | reply classification/rebuttal mapping | broker reply fixtures | shared corpus | byte/semantic | all | TODO |
| DOM-005 | domain | MIME bytes, headers, boundary, CRLF, recipients | fixed time/message ID | raw SMTP fixture | byte | all | TODO |
| DOM-006 | domain | IMAP UIDVALIDITY/HWM/search/fetch policy | fake transcript | state-machine tests | side-effect | all | TODO |
| DOM-007 | domain | OAuth2 state, PKCE, refresh and redaction | mock HTTP server | transcript/files | side-effect | all | TODO |
| DOM-008 | domain | LLM provider descriptors, retries/errors | mock HTTP corpus | transcript tests | side-effect | all | TODO |
| DOM-009 | domain | scheduler bytes/paths/install commands | isolated HOME + fake exec | native snapshots | byte+side-effect | macOS/Linux/Windows | TODO |
| DOM-010 | domain | manual-task evidence/cleanup retention | temp files/DB | filesystem+DB | side-effect | all | TODO |
| MCP-001 | MCP | `initialize` protocol version/capabilities/serverInfo | raw frame | raw frame test | byte | all | TODO |
| MCP-000 | MCP HTTP | bearer secret uses constant-time comparison and strict header parsing | auth corpus; issue #817 | `TestServeHTTPBearerAuthContract` | side-effect | all | PASS |
| MCP-002 | MCP | exact 26-tool `tools/list` | `tools.list.json` | shared golden test | byte | all | TODO |
| MCP-003 | MCP | 26 valid `tools/call` requests | request fixtures | fixture loop | byte/semantic | all | TODO |
| MCP-004 | MCP | success content envelope | handler fixtures | raw frame test | byte | all | TODO |
| MCP-005 | MCP | error codes/messages and SQLite sanitization | malformed corpus | raw frame test | byte | all | TODO |
| MCP-006 | MCP | legacy `list_tools`, `status`, bare `redact_file` | raw corpus | raw frame test | byte | all | TODO |
| MCP-007 | MCP | notifications and mixed/empty batch behavior | raw corpus | raw frame test | byte | all | TODO |
| MCP-008 | MCP | ID types/null/invalid IDs and params | raw corpus | property/fuzz test | byte | all | TODO |
| MCP-009 | MCP HTTP | POST-only, 5 MiB limit, content type | HTTP corpus | HTTP differential | byte+status | all | TODO |
| MCP-010 | MCP HTTP | bearer auth exactness | `internal/mcp/server.go`; issue #817 | `TestServeHTTPBearerAuthContract` | byte+status | all | PASS |
| MCP-011 | MCP HTTP | Origin/loopback/`--allow-remote` policy | host/origin matrix | native network tests | side-effect | native OS | TODO |
| MCP-012 | MCP HTTP | token path/mode/rotation | isolated data dir | filesystem manifest | side-effect | native OS | TODO |
| MCP-013 | MCP HTTP | SIGINT/SIGTERM and 5s graceful shutdown | process harness | signal tests | side-effect | native OS | TODO |
| MCP-014 | MCP stdio | newline frames and zero stdout pollution | raw stream | raw stream comparator | byte | all | TODO |
| MCP-015 | MCP stdio | malformed/truncated/multiple frames | fuzz corpus | parser tests/fuzz | byte+exit | all | TODO |
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
