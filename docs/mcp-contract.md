# MCP Tool Surface Contract

**Status:** pinned · **Tools:** 26 · **Scope:** milestone v1.0.0 (Go port)

This document freezes the MCP tool surface the SwiftUI app
(`app/SymairaEraseMe`) talks to. The Go server is the production
implementation; the pre-cutover implementation is preserved at the
`python-final` tag for historical comparison. Nothing here is invented —
every schema is extracted from and verified against the committed contract.

## 1. Transport and protocol

- Server: **HTTP JSON-RPC 2.0** on `symeraseme mcp` (default port 8000,
  loopback only unless `--allow-remote`).
- Endpoints:
  - `tools/list` → the exact 26-tool definition set from `internal/mcp/tools.json`
    (fixture frozen in
    `tests/fixtures/mcp-contract/tools.list.json`)
  - `tools/call` with `{"name": "<tool>", "arguments": {...}}`
  - legacy bare method `redact_file` (same redaction, returns plain text
    instead of the MCP content envelope)
- JSON-RPC error codes used by the server: `-32700` parse error, `-32600`
  invalid request, `-32601` method not found, `-32602` invalid params /
  missing argument / file-not-found (redaction), `-32603` internal error,
  `-32000` forbidden origin / unauthorized (HTTP status 401/403).
- **Error sanitisation contract:** database internals (sqlite text such as
  `no such table`, `database is locked`, `sqlite3.`) never reach the client.
  `sanitize_client_error` replaces them with
  `"Database not ready — start the server again"`. The Go server must
  implement the same list.

## 2. Response envelope

Every successful `tools/call` returns:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [{ "type": "text", "text": "<CliResult JSON>" }]
  },
  "id": 1
}
```

`<CliResult JSON>` (from `CliResult.to_json()`) has the shape:

```json
{
  "success": true,
  "message": "human readable summary",
  "...data fields": "...spread into the top level"
}
```

Rules:
- `success: true` + `message` + data fields spread at top level.
- `success: false` → `error` string instead of `message`, data fields still
  spread.
- Non-CliResult handler returns are JSON-dumped with `default=str` into the
  same content envelope.

## 3. Tool catalogue (26)

Required parameters are marked `*`.

[MAINTAINED: `internal/mcp/tools.json` plus Go contract tests]

### `redact_file`

Reads a file, runs PII redaction on it, and returns the redacted content.

Required: `['path']`

Parameters:
    - * `path` (string): The path to the file to redact

### `plan_create`

Create a removal campaign plan selecting brokers by jurisdiction and law.

Required: `['campaign_id']`

Parameters:
    - * `campaign_id` (string): Campaign identifier
    -   `jurisdiction` (string): Filter by jurisdiction (e.g. GDPR, CCPA)
    -   `law` (string): Filter by specific law
    -   `priority` (string): Filter by priority level
    -   `category` (string): Filter by broker category (e.g. people-search, marketing)
    -   `max_brokers` (integer): Maximum number of brokers to include
    -   `status` (enum=['active', 'deprecated', 'merged', 'out-of-business']): Broker lifecycle status filter (defaults to active)
    -   `include_inactive` (boolean): Include deprecated, merged, and out-of-business brokers

### `plan_show`

Show the current removal campaign plan.

Required: `—`

Parameters:
    -   `campaign_id` (string): Campaign identifier
    -   `status` (string): Filter by request status

### `execute`

Execute a removal campaign by sending opt-out requests in batches.

Required: `['campaign_id']`

Parameters:
    - * `campaign_id` (string): Campaign identifier
    -   `account` (string): Email account name (himalaya backend)
    -   `batch_size` (integer): Number of requests per batch
    -   `dry_run` (boolean): Preview actions without sending
    -   `backend` (enum=['smtp', 'himalaya']): Email backend to use
    -   `concurrent` (boolean): Use concurrent execution
    -   `workers` (integer): Number of concurrent workers
    -   `consent_token` (string): Consent token value for destructive operations
    -   `consent_file` (string): Path to consent token file

### `poll_inbox`

Poll IMAP inbox for broker replies and match them to removal requests.

Required: `['host', 'port', 'username', 'since_days', 'ssl']`

Parameters:
    - * `host` (string): IMAP server hostname
    - * `port` (integer): IMAP server port
    - * `username` (string): IMAP username (email address)
    - * `since_days` (integer): Fetch messages from the last N days
    - * `ssl` (boolean): Use SSL/TLS connection
    -   `campaign_id` (string): Filter by campaign
    -   `folders` (array): IMAP folders to poll (default: ['INBOX']). Deduplicates by Message-ID across folders.

Implementation details:
- **Adapter**: Production adapter uses `github.com/emersion/go-imap` over TLS 1.2+ (`NetIMAPDialer`).
- **Authentication**: Supports standard IMAP LOGIN as well as SASL XOAUTH2. Passwords and access tokens are resolved safely and never emitted in logs or error strings.
- **Persistent HWM**: High-water marks and `UIDVALIDITY` state are persisted per host and folder in the `imap_state` SQLite table in `symeraseme.db`. UIDVALIDITY changes trigger a cold re-scan.
- **Config Precedence**: Command flags > MCP tool arguments > Environment variables (`IMAP_HOST`, `IMAP_PORT`, `IMAP_USERNAME`, `IMAP_PASSWORD`, `IMAP_SSL`, `IMAP_FOLDER`, `IMAP_SINCE_DAYS`) > Built-in defaults.
- **Deduplication**: When polling multiple folders, replies are deduplicated across folders by RFC 5322 `Message-ID`.
- **Matching & Storage**: Discovered broker replies are correlated against active removal requests and `SENT` events in SQLite, then persisted to `inbox_replies`.

### `classify_reply`

Classify a broker reply using LLM (e.g. confirmation, rejection, info request).

Required: `['request_id']`

Parameters:
    - * `request_id` (integer): Removal request ID
    -   `provider` (string): LLM provider override
    -   `model` (string): LLM model override
    -   `save` (boolean): Save classification to database

### `generate_rebuttal`

Generate a jurisdiction-aware rebuttal for a broker rejection.

Required: `['request_id']`

Parameters:
    - * `request_id` (integer): Removal request ID
    -   `provider` (string): LLM provider override
    -   `model` (string): LLM model override
    -   `save` (boolean): Save rebuttal event to database

### `generate_dashboard`

Generate an HTML dashboard with campaign analytics.

Required: `—`

Parameters:
    -   `output` (string): Output file path
    -   `auto_open` (boolean): Open dashboard in browser after generation
    -   `auto_refresh` (integer): Auto-refresh interval in seconds (0 = disabled)

### `generate_report`

Generate a campaign report in HTML, JSON, or CSV format.

Required: `—`

Parameters:
    -   `campaign_id` (string): Campaign identifier
    -   `format` (enum=['html', 'json', 'csv']): Report format
    -   `output` (string): Output file path
    -   `all_campaigns` (boolean): Include all campaigns

### `manual_tasks_list`

List manual fallback tasks for forms that could not be automated.

Required: `—`

Parameters:
    -   `status` (string): Filter by task status
    -   `request_id` (integer): Filter by request ID

### `manual_tasks_show`

Show details of a specific manual task.

Required: `['task_id']`

Parameters:
    - * `task_id` (integer): Manual task ID

### `manual_tasks_complete`

Mark a manual task as completed.

Required: `['task_id']`

Parameters:
    - * `task_id` (integer): Manual task ID
    -   `notes` (string): Completion notes

### `manual_tasks_cleanup`

Remove old screenshot and HTML snapshot files from manual tasks.

Required: `—`

Parameters:
    -   `dry_run` (boolean): Preview without deleting

### `generate_scheduler`

Generate cron, launchd, or systemd scheduler configurations.

Required: `—`

Parameters:
    -   `platform` (enum=['cron', 'launchd', 'systemd']): Target platform (auto-detected if omitted)
    -   `output_dir` (string): Output directory for config files
    -   `tick_hour` (integer): Hour to run tick engine
    -   `tick_minute` (integer): Minute to run tick engine
    -   `poll_hours` (string): Comma-separated hours for inbox polling
    -   `project_dir` (string): Project directory path
    -   `symeraseme_bin` (string): Path to symeraseme binary
    -   `venv_activate` (string): Virtualenv activate script path
    -   `dry_run` (boolean): Preview without writing files

### `schedule_install`

Generate and install scheduler configurations.

Required: `—`

Parameters:
    -   `platform` (enum=['cron', 'launchd', 'systemd']): Target platform (auto-detected if omitted)
    -   `tick_hour` (integer): Hour to run tick engine
    -   `tick_minute` (integer): Minute to run tick engine
    -   `dry_run` (boolean): Preview without installing

### `schedule_uninstall`

Get instructions for uninstalling scheduler configurations.

Required: `—`

Parameters:
    -   `platform` (enum=['cron', 'launchd', 'systemd']): Target platform (auto-detected if omitted)

### `schedule_status`

Check status of installed scheduler services.

Required: `—`

Parameters:
    -   `platform` (enum=['cron', 'launchd', 'systemd']): Target platform (auto-detected if omitted)

### `validate`

Validate broker registry YAML files against the JSON Schema and Pydantic model.

Required: `—`

Parameters:
    -   `registry_dir` (string): Path to registry directory

### `run_web_form`

Run a broker web-form opt-out through the `symaira-browse` integration.

Required: `['broker_id']`

Parameters:
    - * `broker_id` (string): Broker identifier
    -   `headed` (boolean): Run browser in headed mode (visible)
    -   `screenshot_dir` (string): Directory for screenshots
    -   `dry_run` (boolean): Preview without running

### `auto_confirm`

Auto-click confirmation links in broker reply emails.

Required: `['request_id']`

Parameters:
    - * `request_id` (integer): Removal request ID
    -   `headed` (boolean): Run browser in headed mode
    -   `screenshot_dir` (string): Directory for screenshots
    -   `dry_run` (boolean): Preview without clicking

### `get_dashboard_data`

Return aggregated dashboard data: campaigns, request status counts, broker status, and recent events.

Required: `—`

Parameters:
    - (none)

### `list_requests`

Return paginated removal requests with optional filters.

Required: `—`

Parameters:
    -   `campaign_id` (string): Filter by campaign identifier
    -   `status` (string): Filter by request status
    -   `broker_id` (string): Filter by broker identifier
    -   `page` (integer): 1-indexed page number
    -   `page_size` (integer): Maximum items per page

### `get_events`

Return the event history for a removal request.

Required: `['request_id']`

Parameters:
    - * `request_id` (integer): Removal request ID
    -   `after_event_id` (integer): Only return events with ID greater than this

### `list_brokers`

Return filtered brokers from the registry.

Required: `—`

Parameters:
    -   `jurisdiction` (string): Filter by jurisdiction (e.g. GDPR, CCPA)
    -   `law` (string): Filter by specific law
    -   `priority` (string): Filter by priority level
    -   `category` (string): Filter by broker category
    -   `include_disabled` (boolean): Include disabled brokers
    -   `status` (enum=['active', 'deprecated', 'merged', 'out-of-business']): Broker lifecycle status filter (defaults to active)
    -   `include_inactive` (boolean): Include deprecated, merged, and out-of-business brokers

### `get_calendar`

Return upcoming deadlines and tick actions for the next N weeks.

Required: `—`

Parameters:
    -   `weeks` (integer): Number of weeks to look ahead
    -   `campaign_id` (string): Filter by campaign identifier

### `grant`

Issue, revoke, or list consent tokens for destructive operations.

Required: `—`

Parameters:
    -   `command` (string): Command to grant consent for
    -   `ttl` (integer): Token time-to-live in seconds
    -   `revoke` (string): Token value to revoke
    -   `revoke_all` (boolean): Revoke all active tokens
    -   `list_tokens` (boolean): List all active tokens
    -   `dry_run` (boolean): Preview without issuing or revoking

## 4. Golden fixtures

Directory `tests/fixtures/mcp-contract/`:

- `tools.list.json` — frozen `tools/list` result (exact `internal/mcp/tools.json`).
- `requests/<tool>.request.json` — one valid `tools/call` request per tool,
  maintained as committed contract fixtures.

The Go server's conformance run (milestone v1.0.0) validates that its own
tools/list output equals `tools.list.json` and that every fixture request is
accepted with a well-formed envelope.

## 5. CLI vocabulary decisions (settled before the port)

The MCP tool names (snake_case) and the CLI command names (kebab-case) are
two views over the same operations. Decisions made for v1.0.0:

| MCP tool | CLI command today | Decision |
|---|---|---|
| `plan_create` | `plan create` | keep |
| `plan_show` | `plan show` | keep |
| `execute` | `campaign` group | keep; CLI alias `execute` optional |
| `poll_inbox` | `poll-inbox` | keep |
| `classify_reply` | `classify-reply` | keep |
| `generate_rebuttal` | `generate-rebuttal` | keep |
| `generate_dashboard` | `generate-dashboard` | keep |
| `generate_report` | `generate-report` | keep |
| `generate_scheduler` | `generate-scheduler` | keep |
| `schedule_install` | `schedule install` | keep |
| `schedule_uninstall` | `schedule uninstall` | keep |
| `schedule_status` | `schedule status` | keep |
| `validate` | `registry validate` | keep |
| `run_web_form` | `run-web-form` | keep |
| `auto_confirm` | `auto-confirm` | keep |
| `manual_tasks_list/show/complete/cleanup` | `manual-tasks list/show/complete/cleanup` | keep |
| `get_dashboard_data` | `dashboard` | keep |
| `list_requests` | `requests list` | keep |
| `get_events` | `events show` | keep |
| `list_brokers` | `brokers list` | keep |
| `get_calendar` | `calendar` | keep |
| `grant` | `grant` | keep |
| `redact_file` | `review` / `--interactive` | keep both |
| (server) | `serve --host --port` | **rename to `mcp` at v1.0.0** |

### The one rename: `serve` → `mcp`

- **Decision:** the Go CLI ships `symeraseme mcp` as the server command;
  `symeraseme serve` is retained only as a hidden deprecated alias during the
  cutover.
- **Reason:** `serve` is ambiguous (HTTP server for what?); `mcp` names the
  protocol contractually. The Swift app launches the bundled Go binary with
  `mcp` through `ServerManager`.
- **Transition:** the v1.0.0 Go CLI may keep `serve` as a deprecated alias
  printing a redirect notice for one minor release.

### Accidental shapes noted (fix at v1.0.0 boundary)

- `poll_inbox` requires `host`/`port`/`username`/`ssl` inline — the Go
  server accepts inline params, environment variables (`IMAP_*`), or default values
  (with CLI flags > arguments > env vars > defaults precedence).
- `execute`'s `consent_token`/`consent_file` pair is the destructive-op
  consent mechanism; the Go server keeps both, never prompting from the
  MCP layer (tool calls are non-interactive by design).
- `grant`'s `dry_run` overlaps with `list_tokens` — keep both; they are
  cheap and already shipped.

## 6. Drift protection

The Go contract tests verify the fixture requests and the pinned 26-tool surface:

- `tools.json` is the source for the catalogued tool definitions;
- every fixture request validates against its tool's input contract;
- the tool surface is pinned at 26 names.

Any intended surface change requires updating the committed request fixtures
and the Go contract tests. Schema changes without fixture/test updates fail CI.