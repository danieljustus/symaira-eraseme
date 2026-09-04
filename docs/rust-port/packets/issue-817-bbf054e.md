# Issue 817 context packet

- **Task:** constant-time MCP bearer authentication
- **Base SHA:** `bbf054e3ad9ded806aacb3d1503e8e3b1eda2b99`
- **Allowed files:** `internal/mcp/server.go`, focused MCP auth tests, migration matrix/state
- **Relevant rows:** `MCP-000`, `MCP-010`
- **Acceptance:** strict single `Authorization: Bearer <token>` parsing; constant-time value comparison including length mismatch; unchanged 401 JSON-RPC body and valid-client behavior.
- **Commands:** focused MCP tests, full Go tests/race/lint/vet/build/coverage, focused Swift MCP tests.
- **Safety:** never log or embed a real token.
