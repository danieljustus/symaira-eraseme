# Scoped HTTP source refresh after #1195

Status: capture preparation and native macOS ARM64 no-Go replay verified.
Exact candidate-head six-target CI remains a separate merge gate.

## Immutable producer and measured scope

Producer revision: `5df99f8f5178ba9ae07e6c12be5067ae121e115b`.
Go toolchain: `go1.26.6`. Rust toolchain: `1.98.0`.

The startup-handoff repair changes the recorded `mcp_http_process.rs`
generator. It therefore needs exactly 24 auxiliary records and four wire
records, not a blanket rewrite of the previous refresh:

| Native target | Auxiliary producer | Wire producer |
| --- | --- | --- |
| Linux AMD64 | Actions run `37834833493` | Actions run `37834833576` |
| Linux ARM64 | Actions run `37834833493` | Actions run `37834833576` |
| macOS AMD64 | Actions run `37834833493` | Actions run `37834833576` |
| macOS ARM64 | Unmodified generator on native MacBook | Actions run `37834833576` |

The macOS ARM64 auxiliary Actions job was still waiting for a runner after
the bounded watch. Its six records were instead produced by the unchanged
live Go/Rust generator in a clean standalone Git checkout of the exact
producer revision. All 17 HTTP process tests passed during that capture.
This local capture does not stand in for the candidate's required native CI.

Each selected Actions ZIP was checked against GitHub's artifact SHA-256,
size, owning run and exact head. Each selected JSON record was then checked
against its complete source-file and generator inventories, raw file sizes
and SHA-256 digests, native Go build metadata, and measured byte frames.
The auxiliary observations and all ten wire cases per target exactly match
the previous measured behavior; no response or record was synthesized.

`tests/fixtures/go-frozen/http-refresh-5df99f8f/capture-selection.json`
retains the reviewed per-record byte identities and explicit producer kinds.
Its SHA-256 is
`8a6d3eb4853adb7cfe176991b50b3cb3f04185e1e47b00478ab0f0c35f242db4`.

## Reader and history preservation

Only source-revision, whole-record digest and include-path literals change
in the two HTTP readers. Verifier logic, current-tree binding and negative
controls stay unchanged. All 202 earlier `refresh-411b5c10` records and all
historical originals remain untouched. Unchanged native-bind observations,
including the Windows producer's recorded CRLF behavior, stay selected.
The existing `tests/fixtures/go-frozen/** -text` rule preserves new raw bytes.

The repository's established annotated-tag mechanism retains both original
producer revisions after the squash-only merge:

- Tag: `frozen-go-captures/pr-1194`.
- Tag object: `2a77cbc0357a0284f2d6137d47890d966e18daab`.
- Peeled commit: `5df99f8f5178ba9ae07e6c12be5067ae121e115b`, containing
  `411b5c10eb041f0233e0e86710a3dd733b0012e2` in its history.

This is provenance retention, not a release tag or an acceptance override.
GitHub's squash-only merge setting and all rulesets remain unchanged.

## Local execution and remaining gate

The first linked-worktree capture was rejected by the existing provenance
guard: this host's pinned Go SDK discovered the enclosing repository and
stamped the wrong VCS revision. A separate checkout with its own `.git`
directory produced the correct clean native Go identity. No verifier was
weakened to accept that earlier failed attempt.

After installing the exact 28 new original records and literal reader pins,
the complete 17-test HTTP process suite passed with an explicitly verified
PATH containing no Go executable. Its deliberate corruption, truncation and
fabricated-source probes still failed closed. The frozen-grant privacy test
and `cargo fmt --all -- --check` passed too.

The remaining acceptance gate is full native candidate CI on all six targets
and the regular exact-head PR checks. Retiring Go remains separately gated.
