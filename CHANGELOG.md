# Changelog

All notable changes to this project are documented in this file.

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
