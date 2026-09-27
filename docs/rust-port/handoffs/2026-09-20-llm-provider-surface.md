# Handoff — LLM provider surface (DOM-008)

**Slice:** DOM-008, `internal/llm/{llm,factory,util,agent}.go` → `crates/symeraseme-core/src/llm/mod.rs`.
**Status:** ported and pinned as far as the public API reaches; row DOM-008 stays **PARTIAL** with the
remaining Go ownership named below.

## What is pinned

The oracle `rust-tests/parity/oracle/llm-provider-surface` drives the real Go implementation with a
frozen environment and frozen inputs and writes `tests/fixtures/llm-provider-surface/cases.json`. Two
runs are `cmp`-identical. `crates/symeraseme-core/tests/llm_surface.rs` replays it:

| Section | Cases | Pinned |
|---|---|---|
| `providers` | 5 names | the provider name set (Go returns a map's iteration order, so the fixture pins the sorted set) |
| `usage_records` | 3 | the six-key `Record()` map; `cost` is compared as the number it is, because Go renders an integral `float64` as `0` and serde as `0.0` |
| `cache_key_jitter` | 7 | `fnv32a(key) % 5`, including the empty key's short-circuit to 0 |
| `create_cases` | 8 | resolution order (option → env → default), trim + lowercase, the `agent` model fallback to `auto`, the agent retry/tracker defaults, availability with an empty PATH, and the unknown-provider text with `%q` escaping |
| `retry_cases` | 9 | attempt counts, the returned text/usage, the error text and Go type name, the backoff arithmetic (`2^attempt` plus the cache-key jitter for a rate limit), the foreign-error no-retry path, the cancelled context, `host-agent-unavailable` (retryable `*Error`, three attempts) and the zero-value client's `all 0 retries exhausted: %!w(<nil>)` |

## Rust transport update (2026-09-26)

- EraseMe's Rust LLM adapter now uses CoreKit's `symaira-core-llm` crate, pinned to merge commit
  `0277afe3cf1a35c9db173d9bb29ee07ac6cd6368`. The MCP `classify_reply` and `generate_rebuttal`
  handlers now construct that client through the existing `llm::create` path. Provider descriptors,
  OpenAI/Anthropic wire dialects and HTTP error codes are owned by CoreKit; EraseMe retains its
  existing credential-input, retry and reply-service behavior.
- `crates/symeraseme-core/tests/llmkit_transport_contract.rs` replays the pinned Go HTTP cases, and
  `crates/symeraseme-cli/src/mcp/handler.rs` has a local fake-provider test that exercises both MCP
  consumers without real credentials or external network access. The Go source digests remain pinned
  to the Oracle's CoreKit v0.16.2 behavior.

## Host-agent continuation (2026-09-27)

The host-agent subprocess protocol is now pinned against the real Go implementation with isolated
fake executables for Claude (`claude`), Hermes (`hermes`) and Copilot (`gh`). The replay checks each
argv template, combined prompt, `TERM=dumb`, inherited environment, closed stdin and usage model;
the Claude cases also cover trimmed/truncated exit stderr and the 120-second timeout mapping. The
timeout oracle uses a real 120-second run, so regenerating the complete fixture takes about two
minutes. Rust's focused replay passes all five recorded cases. No real agent CLI or provider is
invoked. Native Windows process behavior remains unverified.

## Traps found while recording

1. `hashCacheKey("")` returns 0 without hashing — the raw `fnv32a("")%5` would have been 1.
2. A rate-limit failure carries an empty message, so `json:"error,omitempty"` dropped the key
   entirely and the fixture claimed a successful call. The record uses `*string` now.
3. The same `omitempty` class of bug hid the `0` values of the agent defaults (`tracker_len`,
   `max_retries` as integers, `available: false`).

## Resume

1. `cargo nextest run -p symeraseme-core -E 'binary(llm_surface)'` — five tests, no network, no sleeps.
2. Re-recording the fixture: `go run ./rust-tests/parity/oracle/llm-provider-surface` (about two
   minutes because it exercises the real 120-second timeout). Never hand-edit the fixture.
3. Remaining DOM-008 work is limited to provider failure cases and native-target coverage; the
   host-agent commands and error paths above are already pinned.
