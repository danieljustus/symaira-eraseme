# Changelog

All notable changes to this project are documented in this file.

## [v0.10.3] - 2026-08-10

- **Fix**: Close SQLite connections cleanly when equivalent database paths use macOS symlink aliases (#672).
- **Test**: Reduce redundant full registry validation in the test suite while retaining authoritative validation coverage (#672).
- **CI**: Correct the TruffleHog configuration for the Lob detector false-positive exclusion (#668, #669).
- **Docs**: Point the Cline example at the repository agent guidance (#667).

## [v0.10.2] - 2026-08-07

- **Fix**: Resolve identity profile decrypt failures, validate email addresses, and honor the configured data directory for identity storage (#648).
- **Fix**: Report accurate doctor environment verdicts for optional checks (#649).
- **Fix**: Format numbers in the macOS app according to the system locale (#650).
- **Fix**: Prevent a nested event loop in plan execute and reject blank campaign ids (#651).
- **Fix**: Keep the Settings Test Connection status in sync with server stop (#652).
- **Fix**: Expose accessible names on sidebar navigation buttons (#653).
- **Fix**: Chain decrypt errors to the user and cover the error paths (#655).
- **Refactor**: Move the web-form sync runner into the service layer for reuse by MCP/app backends (#660).
- **Test**: Cover the concurrent execution branch of `handle_execute` (#661).
- **Test**: Make smoke and tick tests hermetic (#665).
- **Docs**: Add a "Why EraseMe" / comparison section to the README (#662).
- **Chore**: Ignore generated docs/app-test output (#647).

## [v0.10.1] - 2026-08-05

- **Fix**: Ship broker registry data in wheel and sdist so packaged installs can list brokers and create campaigns (#615).
- **Fix**: Default plan creation to active brokers, initialize the database schema at server startup, and sanitize internal database errors (#619, #627).
- **Fix**: Accept Homebrew-installed CLI binaries and show actionable server failure diagnostics (#616, #617).
- **Fix**: Use the configured data directory for MCP authentication and distinguish unauthorized responses from unreachable servers (#618).
- **Fix**: Keep Settings connection state synchronized after server startup (#621).
- **Fix**: Allow Escape to dismiss sheets and size the New Campaign sheet so its primary action remains visible (#622, #623).
- **Fix**: Expose accessible names for sidebar navigation and correct data-directory and calendar escalation labels (#620, #624).
- **Test**: Add regression coverage for server launch diagnostics, fallback interpreter timeouts, packaged registry data, and dashboard/MCP behavior.

## [v0.10.0] - 2026-08-04

- **Feat**: Honor broker lifecycle status in planning and listing (#606).
- **Feat**: Add help descriptions to CLI commands (#607).
- **Feat**: Adopt the shared Symaira app design system for typography, text fields, forms, and accessible status feedback (#584).
- **Fix**: Repair malformed broker website URLs and harden the weekly registry link check (#592).
- **CI**: Restore the maintained Homebrew formula publisher (#590) and halve main-push test cost with macOS gated to weekly (#603).
- **Chore**: Raise the macOS app's `symaira-appkit` dependency to 0.7.0 and bump `cryptography` to 50.0.0 (#591).

## [v0.9.4] - 2026-07-31

- **Fix**: MCP client auth header — read token from server and attach `Authorization: Bearer` to every request (#580).
- **Fix**: ping() parser — correctly parse `tools/list` JSON-RPC response instead of reusing tool-call-envelope decoder (#581).
- **Fix**: Disable locale grouping on server PID display to prevent formatting artifact (#582).
- **Fix**: Surface server stderr in failure banner — show actionable diagnostics instead of only exit code (#583).

## [v0.9.3] - 2026-07-30

- **Fix**: Disable locale grouping on port TextField to prevent formatting artifact (#566).
- **Fix**: Fix MCPClient SIGABRT crash and sidebar engine status indicator (#568).
- **Fix**: Reconcile view model tool names with server and add loading/error states (#569).
- **Feat**: Add Escape key shortcut to dismiss modal sheets (#567).
- **CI**: Fast PR gate — lint + ubuntu tests on PRs, full suite on main + weekly schedule.
- **CI**: Switch from `uv pip audit` to `uv tool run pip-audit` for vulnerability scanning (#558).
- **Chore**: Bump astral-sh/setup-uv from 8.3.2 to 9.0.0 (#571).
- **Chore**: Bump trufflesecurity/trufflehog from 3.95.9 to 3.96.0 (#570).
- **Chore**: Prepare v0.9.2 release — license migration, branding assets, DMG packaging.

## [v0.9.2] - 2026-07-28

- **Fix**: CLI exit codes, help text, and `db-migrate` error message (#555).
- **Fix**: Consume the consent token after verification to enforce a single-use contract (#551).
- **Fix**: Update MCP `serve` auth docs and add MCP schema-sync CI check (#557).
- **Perf**: Persist the IMAP UID high-water mark and batch FETCH commands (#556).
- **Perf**: Short-circuit the registry cache-key walk via directory mtime (#550).
- **CI**: Add a dependency-vulnerability scan with `uv pip audit` (#543, #552), later fixed to use `uv tool run pip-audit` (#558).

## [v0.9.1] - 2026-07-27

- **Fix**: Code-sign and notarize the macOS `.app` release using `notarytool` API-key auth (#541), including a base64 key-decoding fix.

## [v0.9.0] - 2026-07-27

- **Feat**: Migrate the macOS app's `ServerManager` to `SymairaDaemonKit.DaemonSupervisor` (#493).
- **Feat**: Redesign the SwiftUI dashboard and configure DMG release packaging (#490); rename `SymairaDashboard` → `SymairaEraseMe` and migrate to `symaira-appkit` (#491); add MCP dashboard data handlers (#492).
- **Fix**: Redact profile PII in LLM triage prompts (#532); persist the MCP auth token and close a shutdown auth race (#531); fail hard when a `vault://` IMAP password cannot be resolved (#506).
- **Refactor**: Remove the dead orchestrator shim and inbox pass-through (#537); split `himalaya.py` into focused modules (#524, #534); harden and simplify the MCP JSON-RPC server (#505).
- **Perf**: Persist a registry mtime snapshot instead of TTL re-stat (#539); scope inbox poll queries to relevant rows (#538); improve registry loader consistency and performance (#507).
- **CI**: Suppress raw tracebacks with a top-level CLI exception guard (#533); pin the release-app workflow actions to commit SHAs (#530); exclude the inaccessible private Swift dependency from CodeQL (#517).

## [v0.8.0] - 2026-07-08

- **Feat**: Optimize the dashboard UI with Symaira branding and client-side filtering (#488).

## [v0.7.0] - 2026-06-30

- **Feat**: Multi-folder `poll-inbox` and a workflow orchestration template (#481).
- **Fix**: Resolve multiple CLI and execution bugs, and adapt to the Himalaya v1.2 CLI/API breaking changes (#480, #471).
- **Docs**: Add macOS 27 (Tahoe) `pydantic_core` troubleshooting guide and fix script (#455).
- **Chore**: Adopt canonical Apache-2.0 license text (#464).
- **Test**: Add coverage for new modules and service handlers/adapters (#486, #462).

## [v0.6.1] - 2026-06-22

- **Security**: Cap MCP request body size and guard Content-Length parsing to reject oversized payloads (#439).
- **Security**: Refuse non-loopback MCP binds unless `--allow-remote` is explicitly set (#440).
- **Fix**: Use `ThreadingHTTPServer` for the MCP server so concurrent client requests do not block (#444).
- **Fix**: Deduplicate read/redact/error handling blocks in the MCP request handler (#443).
- **Fix**: Route MCP server lifecycle output through Rich console helpers for consistent formatting (#442).
- **Docs**: Document the v0.6.0 MCP server and interactive PII-redaction features in README (#441).

## [v0.6.0] - 2026-06-19

- **Feat**: Add a local MCP JSON-RPC server with a `redact_file` tool for PII redaction workflows (#412).
- **Feat**: Add an interactive terminal review flow for accepting or skipping detected PII redactions (#412).
- **Fix**: Restrict MCP file reads to the server workspace to close CodeQL path-injection findings (#412).
- **Fix**: Add runtime guidance for pydantic_core compatibility failures on macOS 27 (Tahoe) (#413).
- **Fix**: Harden error handling, consent directory permissions, persisted error payloads, broker cache permissions, and domain exception mapping (#425, #437).
- **Fix**: Add JSON output support across plan status, calendar, and broker commands (#437).
- **Perf**: Improve broker cache HMAC handling, YAML metadata parsing, LLM client reuse, and campaign execution threading (#425, #437).
- **CI**: Add macOS test coverage and update pinned GitHub Actions dependencies (#372, #373, #374, #409).
- **Docs**: Expand troubleshooting and exit-code documentation (#425).

## [v0.2.1] – 2026-06-11

- **Security**: Replace pickle with JSON for broker persistent cache to prevent arbitrary code execution (#238).
- **Security**: Fix path traversal via legacy consent token filename, TOCTOU race conditions in consent file verification, and TOCTOU race in encrypted DB open (#283).
- **Security**: Fix doctor command revealing sensitive environment variables, send_reply swallowing KeyboardInterrupt, SQL injection in repository list_replies, and projection.py silently dropping events (#300).
- **Security**: Encrypt existing plaintext DB when SYMERASEME_ENCRYPT_DB=1 (#336).
- **Fix**: Encrypted DB silently discards all writes — use content hash instead of PRAGMA data_version (#344).
- **Fix**: Broker fallback, consent timing, orphan strings, IMAP errors, and doctor redaction tests (#310).
- **Fix**: CliResult envelope, env var redaction, scheduler escaping, and retry docs (#318).
- **Fix**: SIGTERM handler may recursively trigger itself, Windows compatibility gap in secure temp directory creation (#270).
- **Fix**: Orphaned WAL files from encrypted DB temp files not scavenged after crash (#283).
- **Fix**: SIGTERM handler double-calls atexit-registered cleanup, orchestrator deprecation warning fires at import time (#284).
- **Fix**: LLM PII consent check that fails open on unreadable consent file, pin all GitHub Actions workflow steps to full commit SHAs (#328).
- **Fix**: Persistent broker cache not invalidating on YAML edits in subdirectories (#328).
- **Fix**: Skip DB re-encryption on close when no writes occurred (#328).
- **Fix**: Compile JSON Schema once for broker validation instead of per-file (#328).
- **Fix**: Lower default logging level from INFO to WARNING in CLI (#328).
- **Fix**: Add top-level --version flag to CLI (#328).
- **Fix**: Restrict OAuth2 CSRF state file permissions to 0600 (#328).
- **Refactor**: Extract repository layer (campaigns, dashboard, deadlines, events, inbox, manual_tasks, replies, requests) (#271).
- **Refactor**: Extract batch, config, execution, planning, inbox, and exceptions modules from orchestrator (#259, #271).
- **Refactor**: Migrate render_error call sites in services to CliResult(success=False) (#311).
- **Refactor**: Hoist function-local render_error imports to module level (#305).
- **Refactor**: Replace two-query pattern in list_replies with single LEFT JOIN (#284).
- **Refactor**: Limit _prepare_batch to fetch only batch_size rows from database (#284).
- **Refactor**: Build broker ID index from filenames instead of parsing all YAML on cold start (#284).
- **Perf**: Use meta-only YAML parse on cold-cache filter path in load_all_brokers (#312).
- **Perf**: Inbox list fetches envelopes one-by-one — replace with single ranged IMAP fetch (#344).
- **Perf**: PBKDF2 with 600k iterations adds ~0.5s startup overhead for zero security benefit (#336).
- **Feat**: Add web-form fallback adapter for brokers without Playwright support (#260).
- **Feat**: Add doctor command with redaction of sensitive environment variables (#260).
- **Feat**: Add --output json support for several commands (#270).
- **Feat**: Centralize default directory configuration and add writeability checks (#260).
- **Feat**: Remove deprecated top-level CLI shims (execute, tick, status) (#260).
- **Docs**: Add comprehensive AI agent integration support for 10 agents (#229).
- **Docs**: Add scripts/setup-agents.sh for automated agent setup (#229).
- **Chore**: Bump trufflesecurity/trufflehog from 3.95.3 to 3.95.5 (#285).

## [v0.2.0] – 2026-06-01

- Hardened encrypted database storage with per-file salts, safer temporary files, automatic V1-to-V2 migration, and reduced PBKDF2 overhead.
- Improved consent token handling with hashed filenames, atomic `0o600` file creation, and `--consent-file` / `SYMERASEME_CONSENT_FILE` support.
- Fixed batch execution failures, dashboard packaging and permissions, request-event indexing, and campaign dashboard query performance.
- Added profile-aware batch templating, jurisdiction-aware PII scrubber coverage, quieter verbose logging, and faster registry link checks.
- Added Homebrew installation documentation and corrected installation/repository links.
- Refactored scheduler, report generation, service handlers, LLM clients, and registry loading for better maintainability and cold-start performance.

## [v0.1.4] – 2026-05-28

- release: v0.1.4 (#154)

- ci: add check for raw typer.echo error emissions (+3 more) (#153)

- Security fixes: PKCE, DB encryption, PII scrubber, and render_error refactor (+8 more) + merge conflict resolution (#152)

- Separate identity key creation from decryption paths (+2 more) (#139)

- SQL string interpolation fix and 4 more (#129)

- chore(deps): bump actions/checkout from 4 to 6 in /.github/workflows (#122)

- chore(deps): bump actions/setup-python from 5 to 6 in /.github/workflows (#121)

- chore(deps): bump astral-sh/setup-uv from 5 to 7 in /.github/workflows (#120)

- chore(deps): bump actions/github-script from 7 to 9 in /.github/workflows (#119)

- chore(deps): bump peter-evans/create-pull-request from 6 to 8 in /.github/workflows (#123)

- Add CLI help panels, consolidate SMTP, extract render logic, optimize registry loading (+3 more) (#118)

- docs: add terminal demo screenshot to README (#106)

- fix: enable Jinja2 autoescape for defense-in-depth against template injection (#115)

- ci: enforce frozen lockfile in CI and publish workflows (#116)

- test: add integration test suite for CLI-to-event-store path (#117)

- Enable admin bypass protection on main (#107)

- ci: allow manual workflow_dispatch for PyPI publish


## [v0.1.3] – 2026-05-26

- chore(release): bump version to 0.1.3 for PyPI release under new name

- Add GitHub audit report and fix README URLs

- refactor!: rename package from openeraseme to symeraseme

- feat(llm): add generic multi-provider LLM support (#101)


## [v0.1.2] – 2026-05-22

- chore(release): bump version to 0.1.2

- fix: gate no-AAD decryption fallback behind header version check (+8 more) (#100)

- ci: fix TruffleHog secrets-scan failing on push to main

- Consent token files lack restrictive file permissions (+1 more) (#90)

- registry: add 675 new data brokers from research

- fix: update all broker registry entries to use ccpa-deletion template

- fix: rename phantom ccpa-art1798 template to ccpa-deletion in sync script and docs

- docs: update CONTRIBUTING.md integration test description

- fix: add missing __init__.py to llm package

- fix: align __version__ with pyproject.toml (0.1.1)


## [v0.1.1] – 2026-05-21

- chore(release): v0.1.1

- Fix CI failures: remove duplicate broker IDs and fix invalid YAML entries

- Add automated registry maintenance system

- Add metadata fields to all broker entries

- Add 561 new US data broker YAML definitions from state registries

- Security: fix 5 code-scanning alerts (#78)

- Update SECURITY.md (#77)

- chore(pyproject): add project URLs for PyPI sidebar

- style(tests): apply ruff format and remove unused imports

- chore(repo): tighten .gitignore and add audit report


## [v0.1.0] – 2026-05-21

_No conventional commits found._
