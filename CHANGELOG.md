# Changelog

All notable changes to this project are documented in this file.

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
