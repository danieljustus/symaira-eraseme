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
- `../cases/cli/behavior.json` — raw CLI help for every command, plus parser
  unknown-flag/missing-argument cases and successful read-only/completion cases.
- `../cases/mcp/transcript.jsonl` — one fresh `mcp --stdio` process per raw
  JSON-RPC transcript, including 26 schema-valid `tools/call` requests (one
  per pinned tool), batch/notification/legacy/error cases. `stdout_base64`
  and `stderr_base64` preserve bytes; only explicitly listed current-time
  result fields use a marker.
- `../cases/http/transcript.json` — local loopback MCP HTTP method, strict
  bearer-auth, origin, malformed-body, notification, and 5 MiB ceiling cases.
- `../cases/filesystem/manifests.json` — isolated HOME/XDG/TMPDIR side-effect
  manifest, including file modes and an explicit declaration for the random
  MCP token without recording its secret.

All records carry the oracle commit and a schema identifier. Fixture generation
uses UTC, locale `C`, a fixed dedicated `/tmp/symeraseme-go-oracle` runtime
root, an empty private executable search path, empty credential variables, and
no timestamps or host identity. The only
nondeterministic values are marked in `nondeterministic_fields`: the ephemeral
HTTP port, server-issued MCP token, HTTP `Date` header, and current-time fields
in the status/dashboard/calendar CLI and MCP results. The Go `net/http`
`Date` response header is replaced only at `response.headers.Date` with
`<HTTP_DATE>` and is likewise declared as nondeterministic. The oversized HTTP
request is represented by its exact byte length and SHA-256, not stored
verbatim. The token is represented by
`<MCP_TOKEN>` in the request transcript and its file content is never captured.

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
