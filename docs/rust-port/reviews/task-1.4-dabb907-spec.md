# Task 1.4 specification review

- Issue: #803
- Task: 1.4 — Add shadow Rust CI
- Reviewed SHA: `dabb907b56da8d90d743de013e31628f9278bc0f`
- Reviewer lane: specification
- Verdict: **PASS**

## Verified behavior

- Rust 1.98.0 is pinned and the PR gate has no path filter.
- The PR gate runs format, check, Clippy with warnings denied, tests,
  doctests, feature checks, audit, deny, coverage handling and the parity
  target.
- Main/schedule/manual runs add native Ubuntu, macOS and Windows jobs.
- Windows excludes the Unix-only process-tree parity runner and verifies the
  explicit unsupported capability instead of claiming coverage.
- Cargo cache keys include `Cargo.lock` and the pinned toolchain.
- Actions are SHA-pinned, Cargo Dependabot is configured, and existing Go/
  CodeQL workflows and branch-protection contexts are unchanged.
- Actionlint, local Cargo/Go gates, Cargo audit/deny and scope/risk checks pass.

The foundation scaffold has no first-party crypto/consent/auth/MCP modules;
the coverage step reports that capability boundary rather than fabricating a
threshold result. Full-Xcode GUI coverage remains outside this CI task's scope.
