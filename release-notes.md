## What's changed

### Features
- #490 Redesign SwiftUI dashboard & configure DMG release packaging
- #492 Add MCP dashboard data handlers and ignore agent artefacts
- #493 Migrate ServerManager to SymairaDaemonKit.DaemonSupervisor
- #491 Rename SymairaDashboard → SymairaEraseMe and migrate to symaira-appkit

### Fixes
- #531 Persist MCP auth token to file, stop stdout exposure, close shutdown auth race — closes #519
- #532 Apply profile-aware PII redaction in LLM triage pipeline — closes #518
- #533 Suppress raw tracebacks with top-level exception guard — closes #521, #522, #523, #525
- #506 Fail hard when a vault:// IMAP password cannot be resolved — closes #500
- #530 Pin release-app workflow actions to commit SHAs — closes #520

### Refactors
- #534 Split himalaya.py into himalaya_config + smtp_himalaya modules — closes #524, #527
- #537 Remove dead orchestrator shim and inbox pass-through — closes #526

### Performance
- #538 Scope inbox poll queries to relevant rows — closes #528
- #539 Persist registry mtime snapshot instead of TTL re-stat — closes #529
- #507 Improve registry loader consistency and performance — closes #502, #503

### Security
- #505 Harden and simplify the MCP JSON-RPC server — closes #499, #501, #504

### Tests
- #513 Add unit coverage for the dashboard service handlers — closes #508
- #514 Add unit coverage for the core orchestrator compatibility layer — closes #509
- #515 Add unit coverage for the reporting service handlers
- #516 Speed up watcher and Anthropic retry unit tests — closes #511, #512

### CI & Dependencies
- #517 Exclude inaccessible private Swift dependency from CodeQL
- #535 Bump actions/checkout 7.0.0 → 7.0.1
- #536 Bump actions/setup-python 6.3.0 → 7.0.0
- #494–#498 Bump minor-patch dependency groups

### Closed Issues
- #499 MCP server security hardening
- #500 vault:// IMAP password resolution failure
- #501 MCP auth token lifecycle
- #502 Registry loader consistency
- #503 Registry loader performance
- #504 MCP JSON-RPC simplification
- #508 Dashboard service test coverage
- #509 Orchestrator compatibility test coverage
- #511 Watcher test performance
- #512 Anthropic retry test performance
- #518 LLM triage PII redaction
- #519 MCP auth token persistence
- #520 Release workflow action pinning
- #521 CLI exception guard
- #522 CLI help docstrings
- #523 Plan create --max truncation warning
- #524 himalaya.py module split
- #525 Calendar SQL extraction
- #526 Dead orchestrator shim removal
- #527 IMAP poll efficiency
- #528 Inbox poll query scoping
- #529 Registry cache mtime snapshot

**Full Changelog**: https://github.com/danieljustus/symaira-eraseme/compare/v0.8.0...v0.9.0
