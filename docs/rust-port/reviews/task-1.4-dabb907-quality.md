# Task 1.4 quality and security review

- Issue: #803
- Task: 1.4 — Add shadow Rust CI
- Reviewed SHA: `dabb907b56da8d90d743de013e31628f9278bc0f`
- Reviewer lane: quality/security
- Verdict: **APPROVED**

## Verified gates

- `actionlint .github/workflows/rust-ci.yml` — exit 0
- Rust 1.98.0 toolchain and pinned Cargo verification tools are explicit
- All workflow actions use verified commit SHAs
- Workflow permissions are contents-read only
- PR gate has no path filter; existing Go/CodeQL workflows and required
  contexts are unchanged
- Cargo cache key includes `Cargo.lock` and `rust-toolchain.toml`
- Windows excludes the Unix-only parity process-tree runner and asserts the
  explicit unsupported capability
- `cargo audit`, `cargo deny`, feature checks and local Go/Rust/Parity gates pass
- Added-line secret/path/scope checks are clear

## Informational residuals

The foundation coverage step uses an explicit source-name activation check and
reports no critical-module claim while those modules do not exist. The Windows
capability assertion is a static guard for the documented unsupported process
cleanup boundary; actual Windows Job Object support remains future work. Both
limitations fail closed or remain advisory and do not weaken the current PR
or native Rust gates.
