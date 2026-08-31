# SQLite Event Store Contract

**Status:** pinned · **Schema version:** `1` (PRAGMA user_version) · **Scope:** milestone v1.0.0 (Go port)

This document specifies the on-disk format of the Symaira EraseMe event
store so the Go port can read and write the same database files used by the
pre-cutover implementation. The archived implementation is available at the
`python-final` tag. The golden fixture
(`tests/fixtures/event-store/golden-campaign.db` + `golden-projection.json`)
is the executable part of this contract: the Go projection must reproduce
the exact JSON from the exact SQLite file.

## 1. Database file

- SQLite 3, one file: `~/.local/share/symeraseme/symeraseme.db`
  (`SYMERASEME_DB_DIR` overrides the directory, `SYMERASEME_DATA_DIR` the
  base). The Go port must accept the same two env vars.
- `PRAGMA user_version = 1` marks the schema version; `init_db()` creates
  missing tables idempotently and refuses to operate below its expected
  version only by not downgrading (it never migrates down).
- **Timestamps are UTC text** in one of three parseable forms (see
  `core/datetime_utils.py`):
  - `%Y-%m-%dT%H:%M:%S` (no tz — interpreted as UTC)
  - `%Y-%m-%dT%H:%M:%S%z`
  - `%Y-%m-%d %H:%M:%S` (SQLite `datetime('now')` output)
  Rules: stored via `datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")` for
  events; column defaults use `datetime('now')` (space separator). Both are
  valid; parsers must accept all three.
- No foreign-key enforcement beyond declaration (`PRAGMA foreign_keys` is
  not enabled by default; the app relies on application logic).

## 2. Tables involved in the event-sourced path

### campaigns

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PRIMARY KEY | campaign identifier |
| `created_at` | TIMESTAMP NOT NULL | default `datetime('now')` |
| `kind` | TEXT NOT NULL | default `'initial'` |
| `notes` | TEXT | nullable |

### removal_requests

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY AUTOINCREMENT | request id, referenced everywhere |
| `broker_id` | TEXT NOT NULL | registry broker id |
| `channel` | TEXT NOT NULL | default `'email'`; values `email` / `web_form` in practice |
| `campaign_id` | TEXT NOT NULL | FK to campaigns.id |
| `created_at` | TIMESTAMP NOT NULL | default `datetime('now')` |
| `jurisdiction` | TEXT NOT NULL | e.g. `US`, `DE`, `EU` … (closed enum of registry contract) |
| `template_id` | TEXT NOT NULL | default `''`; `ccpa-deletion` / `gdpr-art17` in practice |
| `identity_snapshot_hash` | TEXT NOT NULL | default `''`; sha-256 of the identity snapshot used |

### request_events (the append-only log)

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY AUTOINCREMENT | **global** event id (not per request) |
| `request_id` | INTEGER NOT NULL | FK to removal_requests.id |
| `occurred_at` | TIMESTAMP NOT NULL | business time of the event (may be backdated) |
| `recorded_at` | TIMESTAMP NOT NULL | ingestion time |
| `event_type` | TEXT NOT NULL | closed catalogue, §3 |
| `payload_json` | TEXT NOT NULL | `'{}'` default; JSON object of event-specific fields |
| `source` | TEXT NOT NULL | `system` / `inbox` / `user` / `scheduler` |

Ordering rule for replay: `ORDER BY occurred_at ASC, id ASC` — the id
tie-breaker makes same-instant events deterministic.

### request_state (the projection)

| Column | Type | Notes |
|---|---|---|
| `request_id` | INTEGER PRIMARY KEY | FK |
| `current_status` | TEXT NOT NULL | derived, §4 |
| `last_event_id` | INTEGER NOT NULL | max replayed event id |
| `last_event_at` | TIMESTAMP | occurred_at of last event |
| `sent_at` / `acknowledged_at` / `resolved_at` | TIMESTAMP | set by SENT / ACK / CONFIRMED·REJECTED_FINAL |
| `deadline_at` | TIMESTAMP | SENT + payload `expected_response_days` |
| `next_action_at` | TIMESTAMP | used by tick engine; always settable by caller code (never by events themselves) |
| `reminders_sent` | INTEGER NOT NULL | from REMINDER_SENT payload `count` (or 1) |
| `escalation_level` | INTEGER NOT NULL | DEADLINE_REACHED→1, DPA_COMPLAINT_DRAFTED→2 |

The projection is **derived, not authoritative**: `RebuildState` replays
the log and recomputes every column; `UpsertState` writes it;
`append_event_and_project()` writes event+projection atomically in one
transaction. `rebuild_all_states()` rebuilds dirty rows in chunks of 100.
The golden fixture ships with an **empty** `request_state` on purpose.

## 3. Event type catalogue

Closed set (validation in `core/events.py` `EVENT_TYPES`):

`PLANNED, SENT, SEND_FAILED, BOUNCE, AUTORESPONDER, ACK, VERIFICATION_REQUESTED, VERIFICATION_PROVIDED, HUMAN_ACTION_REQUIRED, CONFIRMATION_LINK_CLICKED, REPLY_DRAFTED, REBUTTAL_SENT, REMINDER_SENT, DEADLINE_REACHED, DPA_COMPLAINT_DRAFTED, DPA_COMPLAINT_FILED, CONFIRMED, REJECTED_FINAL, RE_SCAN_TRIGGERED, NOTE_ADDED`

Payload fields observed/used by the projection (all optional JSON):

- **SENT**: `expected_response_days` (int, default 30 — drives `deadline_at`), `broker_id`
- **REMINDER_SENT**: `count` (int — `reminders_sent = payload.count or 1`)
- **PLANNED**: `campaign_id` (informational)
- **ACK**: `message_id` (informational)
- others: informational `via`, `reason`, `authority`, `broker_id`, `campaign_id`

Unknown event types or sources must be rejected on append
(`ValueError`); **unknown event types encountered during replay are logged
and skipped** (forward compatibility, see §7).

## 4. Status transition function

| Event | → current_status |
|---|---|
| PLANNED | PLANNED |
| SENT | AWAITING_ACK |
| SEND_FAILED | SEND_FAILED |
| BOUNCE | BOUNCE |
| ACK | ACK |
| AUTORESPONDER | AWAITING_ACK |
| VERIFICATION_REQUESTED | AWAITING_USER_ACTION |
| VERIFICATION_PROVIDED | AWAITING_RESPONSE |
| HUMAN_ACTION_REQUIRED | AWAITING_USER_ACTION |
| CONFIRMED | CONFIRMED |
| REJECTED_FINAL | REJECTED_FINAL |
| CONFIRMATION_LINK_CLICKED | CONFIRMED |
| REBUTTAL_SENT | AWAITING_RESPONSE |
| REMINDER_SENT | AWAITING_ACK |
| DEADLINE_REACHED | OVERDUE |
| DPA_COMPLAINT_DRAFTED | ESCALATED |
| DPA_COMPLAINT_FILED | DPA_FILED |
| RE_SCAN_TRIGGERED | RE_SCAN_DUE |
| NOTE_ADDED | (no change) |

Side effects per event during replay:
- SENT → `sent_at = occurred_at`, `deadline_at = occurred_at + expected_response_days` (days)
- ACK → `acknowledged_at = occurred_at`
- CONFIRMED / REJECTED_FINAL → `resolved_at = occurred_at`
- REMINDER_SENT → `reminders_sent = payload.count or 1`
- DEADLINE_REACHED → `escalation_level = 1`
- DPA_COMPLAINT_DRAFTED → `escalation_level = 2`

Replay is a fold left-to-right over `ORDER BY occurred_at ASC, id ASC`;
later events overwrite earlier statuses (no monotonicity check).

## 5. Encryption envelope (highest-risk parity area)

The whole database file may be encrypted at rest when
`SYMERASEME_ENCRYPT_DB` is `1|true|yes`. **The file format is Fernet
(AES-256-GCM), not raw SQLite**, and decrypt-on-open / encrypt-on-close is
transparent.

Three header versions exist; current write version is **V3**:

| Version | Header | Layout after header |
|---|---|---|
| V1 (legacy, still readable) | `SYMERASEME_ENCv1\n` (15 bytes) | Fernet token only (no salt in file); uses fixed PBKDF2 salt |
| V2 | `SYMERASEME_ENCv2\n` (16 bytes) + 16-byte random salt | Fernet token, key = PBKDF2-HMAC-SHA256(master, salt, 600_000) |
| V3 (current) | `SYMERASEME_ENCv3\n` (16 bytes) + 16-byte random salt | Fernet token, key = HKDF-SHA256(master, salt=salt, info=`symeraseme-db-encryption-v3`) |

Key derivation:
- master key = 32-byte identity master key (keyring/OS keychain, never on
  disk unencrypted).
- V1/V2: `PBKDF2-HMAC-SHA256(password=master, salt=fixed-or-random(16), iterations=600_000, dklen=32)` → URL-safe base64 → Fernet key.
- V3: `HKDF-SHA256(ikm=master, salt=random(16), info="symeraseme-db-encryption-v3", length=32)` → URL-safe base64 → Fernet key.
- V1 has no per-file salt (fixed `symeraseme-db-encryption-v1`); V2/V3 draw `secrets.token_bytes(16)` per write.

Migrations on open (transparent, log only):
- V1 file → `_migrate_v1_to_v2` → `_migrate_v2_to_v3`
- V2 file → `_migrate_v2_to_v3`

Encrypted-file detection: file begins with the V1 header **or** `V2/V3
magic + salt` prefix. Decrypted copies live in `$TMPDIR/symeraseme-db-<uid>/`
(`/dev/shm/symeraseme-db-<uid>` on Linux), mode 0600, and are re-encrypted
on connection close. `SYMERASEME_ENCRYPT_DB` unset/`0` → plain SQLite file.

Field-vs-blob scope: encryption is **whole-file**, not per-field. The Go
port must implement the identical V3 write path (random salt, HKDF,
Fernet/AES-256-GCM token construction) or user data is unreadable.

## 6. Golden fixture usage

```
tests/fixtures/event-store/
  golden-campaign.db        committed SQLite file, request_state EMPTY
  golden-projection.json    expected rebuilt projections (keyed by request id)
```

The golden fixtures are maintained as committed data. Run the Go conformance
tests after changing the event-store contract:
`go test ./internal/eventstore/...`

Conformance: `internal/eventstore/conformance_test.go`
1. copies the golden DB to a temp path, opens it with the Go store, rebuilds
   state per request, and compares **exactly** against `golden-projection.json`;
2. asserts `request_state` is empty before rebuild (projection is derived);
3. asserts the fixture covers CONFIRMED / REJECTED_FINAL / ESCALATED paths +
   deadline derivation + reminder counter + escalation level.

The Go conformance test replays the same file and must produce the same
JSON bytes.

## 7. Implicit behaviours (traps for the port)

- `datetime('now')` defaults are UTC but space-separated; the Go parser accepts
  both space- and `T`-separated timestamps.
- Replay ordering uses `(occurred_at, id)`; payload `null`/`''` must be
  treated as `{}`.
- Unparseable timestamps or payloads during replay are logged and skipped by
  the Go projection (never aborting the rebuild).
- `next_action_at` is never written by events; it is tick-engine bookkeeping
  and may be any value.
- `request_state.reminders_sent` default is `0`; first REMINDER_SENT with no
  `count` payload sets it to `1`.
- AUTOINCREMENT ids are global across requests; do not assume per-request
  sequencing.
- `SOURCE` validation is on append only; replay ignores sources.
- The `request_state` table also receives `INSERT OR REPLACE` from
  `upsert_state` with **11 named columns in a fixed order** — the Go port
  must match column-for-column (missing `next_action_at` insert would
  reorder silently).