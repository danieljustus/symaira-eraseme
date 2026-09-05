# Go oracle fixtures

These fixtures are generated from corrected Go commit `bf53346eec234929bedf0314b99e3da85dbb991b` by
`scripts/generate-go-oracle-fixtures.sh`. The generator exports that commit with
`git archive`, injects one untracked `_test.go` beside the real `newRootCommand`,
and launches the resulting binary as a black box. No tracked Go source is
modified and no developer profile, keychain, database, or credential is read.

## Layout

- `../cases/cli/surface.json` — recursively serialized Cobra metadata. `root`
  and every child include canonical path, public ordering, hidden/deprecated
  status, aliases, positional `Use` forms, validator function, local,
  inherited, persistent, and effective flags with defaults and order.
- `../cases/cli/behavior.json` — raw CLI help and unknown-flag behavior for
  every command, root version/unknown-command cases, true parser-level missing
  arguments, and one isolated operational invocation for every executable leaf.
  Each operational record classifies success versus deterministic backend error.
- `../cases/mcp/transcript.jsonl` — one fresh `mcp --stdio` process per raw
  JSON-RPC transcript, including 26 schema-valid `tools/call` requests (one
  per pinned tool), ID/params, multi-frame, truncation, shutdown,
  batch/notification/legacy/error cases. `stdout_base64` and `stderr_base64`
  preserve bytes; only explicitly listed current-time result fields use a marker.
- `../cases/http/transcript.json` — MCP HTTP bind/remote policy, method,
  content type, Host, strict bearer auth, origin, malformed body, notification,
  exact 5 MiB boundary, and oversized-body cases using local listeners only.
- `../cases/filesystem/manifests.json` — isolated HOME/XDG/TMPDIR side-effect
  manifests for profile creation, consent, scheduler/report output, durable
  manual fallback, migration, and MCP-token rotation. Random/private bytes are
  represented only by path-specific nondeterminism declarations.

All records carry the oracle commit and a schema identifier. Fixture generation
uses Go `go1.26.6` resolved with `GOTOOLCHAIN=go1.26.6` and `GOPROXY=off`,
`GOENV=off`, `GOWORK=off`, an isolated `HOME`/build cache, and only the pinned
module cache. It also uses UTC, locale `C`, an empty private executable search
path, empty credential variables, and no host identity. The runtime root is a
unique `mktemp` directory owned by this invocation. Only that exact root is
replaced with `<ORACLE_ROOT>` in argv/stdout/stderr/manifests and artifact
content; this narrow rule is declared in each corpus document's `normalization`.
The only other nondeterministic values are marked in `nondeterministic_fields`:
ephemeral ports, server-issued MCP/consent tokens, encrypted-profile nonces,
private or time-derived durable artifacts, HTTP `Date`, and current-time fields
in status/dashboard/calendar/report results. Report HTML retains normalized
content and SHA-256 evidence. Consent output is checked to contain neither the
issued token nor known time-derived fields. The oversized HTTP request is
represented by its exact byte length and SHA-256, not stored verbatim. The MCP
token is represented by `<MCP_TOKEN>` in the request transcript and its file
content is never captured. The remote-policy allow probe accepts policy for
TEST-NET `192.0.2.1` and then records the expected OS bind failure; it never
binds `0.0.0.0` or a LAN interface.

The generator builds a complete same-filesystem staging corpus and swaps
`cases` and `fixtures` only after all generation and count checks pass. Any
failure leaves the previous corpus byte-identical and rollback restores it if a
swap is interrupted.

The generator is an executable drift gate:

```bash
scripts/generate-go-oracle-fixtures.sh
cp -R rust-tests/parity/cases /tmp/oracle-cases-first
scripts/generate-go-oracle-fixtures.sh
diff -ru /tmp/oracle-cases-first rust-tests/parity/cases
```

The second generation must be byte-identical. Any intended Go contract change
must first be corrected in Go, then regenerated from the new pinned commit;
fixtures must not be hand-edited.
