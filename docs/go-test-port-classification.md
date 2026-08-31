# Python test classification for the Go port

This inventory records the 74 Python test files that existed before the cutover.
The files are removed by #731; the table preserves the review decision and the
replacement evidence for future audits.

## Classification

- **Ported** — behavior is covered by Go unit/integration tests for the
  corresponding package.
- **Contract replacement** — the Python test asserted a wire/schema/packaging
  contract; the Go implementation now owns that contract and tests it directly.
- **Remove with #731** — Python-only infrastructure or provider adapter; it
  was removed in this cutover because the Go runtime has no corresponding
  surface.

| Python test | Classification | Go evidence or removal rationale |
|---|---|---|
| `tests/integration/test_critical_path.py` | Ported | `internal/campaign`, `internal/eventstore`, `internal/replies` integration paths |
| `tests/registry/test_packaging.py` | Contract replacement | `internal/registry/loader_test.go`, embedded registry validation |
| `tests/smoke/test_action_skill.py` | Remove with #731 | Python skill orchestration smoke; no Go runtime surface |
| `tests/smoke/test_broker_validation.py` | Contract replacement | `internal/registry` schema and validation tests |
| `tests/smoke/test_cli_commands.py` | Ported | `cmd/symeraseme` command-surface and CLI smoke tests |
| `tests/smoke/test_plan_skill.py` | Ported | CLI plan commands and `internal/campaign` tests |
| `tests/smoke/test_send_skill.py` | Ported | `internal/replies` and email transport tests |
| `tests/smoke/test_setup_skill.py` | Ported | `internal/identity` bootstrap/consent tests |
| `tests/smoke/test_tick_skill.py` | Ported | `internal/scheduler` tests |
| `tests/smoke/test_triage_skill.py` | Ported | `internal/triage` classifier/rebuttal tests |
| `tests/smoke/test_web_form_and_inbox.py` | Ported | `internal/campaign` formflow and `internal/email` tests |
| `tests/unit/test_agent_client.py` | Contract replacement | `internal/llm` shared client contract tests |
| `tests/unit/test_anthropic_adapter.py` | Contract replacement | Shared provider contract is tested through `internal/llm` |
| `tests/unit/test_anthropic_client.py` | Contract replacement | Shared provider contract is tested through `internal/llm` |
| `tests/unit/test_auto_confirm_service.py` | Ported | `internal/confirmation`, `internal/identity`, and reply lifecycle tests |
| `tests/unit/test_broker_lifecycle.py` | Ported | `internal/registry` and `internal/campaign` lifecycle tests |
| `tests/unit/test_captcha_service.py` | Ported | `internal/campaign` formflow/CAPTCHA contract tests |
| `tests/unit/test_captcha_solver.py` | Ported | `internal/campaign` formflow solver paths |
| `tests/unit/test_classifier.py` | Ported | `internal/triage` classifier tests |
| `tests/unit/test_cli.py` | Ported | `cmd/symeraseme` root-command tests |
| `tests/unit/test_cli_args.py` | Ported | `cmd/symeraseme` positional argument and flag tests |
| `tests/unit/test_cli_commands.py` | Ported | `cmd/symeraseme` command-surface tests |
| `tests/unit/test_cli_result_envelope.py` | Contract replacement | Go CLI JSON result and error envelope tests |
| `tests/unit/test_config.py` | Ported | `internal/config` and CLI configuration paths |
| `tests/unit/test_confirmation_clicker.py` | Ported | `internal/confirmation` tests |
| `tests/unit/test_consent.py` | Ported | `internal/identity` consent/token tests |
| `tests/unit/test_consent_security.py` | Ported | `internal/identity` secure token and path tests |
| `tests/unit/test_dashboard.py` | Ported | `internal/reporting` dashboard tests |
| `tests/unit/test_dashboard_service.py` | Ported | `internal/reporting` service tests |
| `tests/unit/test_db_connection.py` | Contract replacement | `internal/eventstore` database lifecycle tests |
| `tests/unit/test_db_encryption.py` | Ported | `internal/eventstore` encryption envelope tests |
| `tests/unit/test_deadlines.py` | Ported | `internal/deadlines` tests |
| `tests/unit/test_doctor_redaction.py` | Ported | `internal/redaction` tests |
| `tests/unit/test_event_store.py` | Ported | `internal/eventstore` repository/projection tests |
| `tests/unit/test_event_store_contract.py` | Contract replacement | `internal/eventstore` conformance tests |
| `tests/unit/test_events_facade.py` | Ported | `internal/eventstore` facade tests |
| `tests/unit/test_himalaya.py` | Remove with #731 | Python Himalaya subprocess adapter; Go uses its own email transport |
| `tests/unit/test_identity.py` | Ported | `internal/identity` profile and key tests |
| `tests/unit/test_imap.py` | Ported | `internal/email` IMAP parsing/polling tests |
| `tests/unit/test_inbox_service.py` | Ported | `internal/email` inbox service tests |
| `tests/unit/test_interactive.py` | Remove with #731 | Python-only interactive shell behavior |
| `tests/unit/test_llm_factory.py` | Contract replacement | `internal/llm` provider factory contract |
| `tests/unit/test_llm_protocol.py` | Contract replacement | `internal/llm` protocol/error tests |
| `tests/unit/test_manual_fallback.py` | Ported | `internal/manualtasks` fallback/instruction tests |
| `tests/unit/test_manual_task_service.py` | Ported | `internal/manualtasks` service tests |
| `tests/unit/test_mcp_contract.py` | Contract replacement | `internal/mcp` 26-tool contract tests |
| `tests/unit/test_mcp_server.py` | Contract replacement | `internal/mcp` HTTP/stdio transport tests |
| `tests/unit/test_mcp_server_handler.py` | Contract replacement | `internal/mcp` dispatch/error tests |
| `tests/unit/test_notify.py` | Remove with #731 | Python-only notification integration |
| `tests/unit/test_oauth2.py` | Remove with #731 | Python-only OAuth helper; no Go runtime surface in the port milestone |
| `tests/unit/test_ollama_client.py` | Contract replacement | Shared provider contract in `internal/llm` |
| `tests/unit/test_openai_client.py` | Contract replacement | Shared provider contract in `internal/llm` |
| `tests/unit/test_openai_compatible_client.py` | Contract replacement | Shared provider contract in `internal/llm` |
| `tests/unit/test_orchestrator.py` | Remove with #731 | Python orchestration implementation; replaced by Go CLI/MCP entrypoints |
| `tests/unit/test_registry_contract.py` | Contract replacement | `internal/registry` loader/schema tests |
| `tests/unit/test_registry_sync.py` | Ported | `internal/registry` loader and embedded data tests |
| `tests/unit/test_reply_manager.py` | Ported | `internal/replies` lifecycle tests |
| `tests/unit/test_reply_service.py` | Ported | `internal/replies` service/repository tests |
| `tests/unit/test_reporting_service.py` | Ported | `internal/reporting` tests |
| `tests/unit/test_reports.py` | Ported | `internal/reporting` report generation tests |
| `tests/unit/test_repositories.py` | Ported | `internal/eventstore` and `internal/replies` repositories |
| `tests/unit/test_responder.py` | Ported | `internal/triage` responder tests |
| `tests/unit/test_scheduler.py` | Ported | `internal/scheduler` generation/install tests |
| `tests/unit/test_scheduler_service.py` | Ported | `internal/scheduler` service tests |
| `tests/unit/test_schema.py` | Contract replacement | Go registry/eventstore schema validation |
| `tests/unit/test_scrubber.py` | Ported | `internal/redaction` scrubber tests |
| `tests/unit/test_scrubber_consent.py` | Ported | `internal/redaction` and `internal/identity` consent tests |
| `tests/unit/test_secrets.py` | Ported | `internal/identity` key/secret resolution tests |
| `tests/unit/test_templating.py` | Ported | `internal/templating` parity tests |
| `tests/unit/test_validate_service.py` | Ported | `internal/registry` validation tests |
| `tests/unit/test_watcher.py` | Remove with #731 | Python-only watcher process |
| `tests/unit/test_web_fallback.py` | Ported | `internal/campaign` browser/form fallback contract |
| `tests/unit/test_web_form.py` | Ported | `internal/campaign` formflow tests |
| `tests/unit/test_web_form_service.py` | Ported | `internal/campaign` form service tests |

## Gate and follow-up

The local reference run covers 5,184 / 6,842 statements = 75.77%. CI remains
canonical for the runner's target OS; the exact profile gate is evaluated there
rather than trusting rounded package summaries.

`make coverage` enforces `COVERAGE_THRESHOLD ?= 75` using the profile's exact
statement counts. The Python test files listed above are now archived decisions;
#731 removed the Python CI leg and implementation after the Go gate passed.
