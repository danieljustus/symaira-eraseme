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

## What stays Go, and why

- `anthropic`, `openai`, `ollama`, `openai-compatible`: transports, wire dialects, credential
  reference resolution and the `auth_failure` text all live in `corekit/llmkit`, which has no Rust
  counterpart. `llm::create` reports those providers as `TransportNotPorted` instead of pretending an
  equivalent client exists; the fixture keeps the measured Go text of that path under `boundaries`.
- The host-agent subprocess protocol: CLI **detection** is ported (PATH lookup with the executable
  bit, `agentDefs`, preference order), but the invocation template, the 120 s timeout and the
  exit-code wrapping need a fake CLI on PATH to pin. `AgentClient::unavailable_error` covers the one
  agent failure that needs no subprocess.

## Traps found while recording

1. `hashCacheKey("")` returns 0 without hashing — the raw `fnv32a("")%5` would have been 1.
2. A rate-limit failure carries an empty message, so `json:"error,omitempty"` dropped the key
   entirely and the fixture claimed a successful call. The record uses `*string` now.
3. The same `omitempty` class of bug hid the `0` values of the agent defaults (`tracker_len`,
   `max_retries` as integers, `available: false`).

## Resume

1. `cargo nextest run -p symeraseme-core -E 'binary(llm_surface)'` — five tests, no network, no sleeps.
2. Re-recording the fixture: `go run ./rust-tests/parity/oracle/llm-provider-surface` (takes ~9 s, the
   retry cases really wait out their backoff). Never hand-edit the fixture.
3. Next honest step for this row: a fake host-agent CLI on an isolated PATH, so the invocation
   template, the timeout and the exit-code error text can be pinned too.