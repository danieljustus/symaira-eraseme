// Package eventstore (store.go) is the Go port of the Python
// symeraseme.core.eventstore + db_connection. It exposes a small Store
// facade backed by pure-Go SQLite (modernc.org/sqlite, no CGO) and
// offers append + projection primitives that match the contract in
// docs/event-store.md.
package eventstore

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"time"

	// Pure-Go SQLite driver (no CGO).  Registered as the "sqlite"
	// driver name for database/sql.
	_ "modernc.org/sqlite"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore/timeutil"
)

// SchemaVersion is the user_version pragma value written by InitSchema.
const SchemaVersion = 1

// DBFileName is the canonical database file name inside the data dir.
const DBFileName = "symeraseme.db"

// ErrUnknownEventType is returned by Append when the event type is
// not in ValidEventTypes.
var ErrUnknownEventType = errors.New("eventstore: unknown event type")

// ErrUnknownSource is returned by Append when the source is not in
// ValidSources.
var ErrUnknownSource = errors.New("eventstore: unknown source")

// Event is a stored request_events row, materialised for use by the
// projection fold. Payload is the JSON object (never nil/empty in
// memory — the projection treats "" as {}).
type Event struct {
	ID         int64
	RequestID  int64
	OccurredAt time.Time
	RecordedAt time.Time
	EventType  EventType
	Payload    map[string]any
	Source     Source
}

// rawEvent is the on-disk shape returned by the SELECTs; payload is
// still raw JSON.
type rawEvent struct {
	ID         int64
	RequestID  int64
	OccurredAt string
	RecordedAt string
	EventType  string
	Payload    string
	Source     string
}

// toEvent lifts a rawEvent into the projection-friendly Event.
func (r rawEvent) toEvent() (Event, error) {
	occurred, err := timeutil.Parse(r.OccurredAt)
	if err != nil {
		return Event{}, err
	}
	recorded, err := timeutil.Parse(r.RecordedAt)
	if err != nil {
		return Event{}, err
	}
	payload := map[string]any{}
	if strings.TrimSpace(r.Payload) != "" {
		if err := json.Unmarshal([]byte(r.Payload), &payload); err != nil {
			return Event{}, fmt.Errorf("eventstore: bad payload for id=%d: %w", r.ID, err)
		}
		if payload == nil {
			payload = map[string]any{}
		}
	}
	return Event{
		ID:         r.ID,
		RequestID:  r.RequestID,
		OccurredAt: occurred,
		RecordedAt: recorded,
		EventType:  EventType(r.EventType),
		Payload:    payload,
		Source:     Source(r.Source),
	}, nil
}

// Store is a connection-pool facade over a SQLite database file. It
// is safe for concurrent use: database/sql's connection pool handles
// per-goroutine connection reuse. The encryption envelope is handled
// by OpenEncrypted + WriteEncrypted in encrypt.go.
type Store struct {
	db   *sql.DB
	path string
}

// Path returns the resolved file path of the database (may be a
// decrypted temp file when encryption is enabled).
func (s *Store) Path() string { return s.path }

// DB returns the underlying *sql.DB handle. Exposed for repository
// callers that need a raw handle; prefer the Store methods.
func (s *Store) DB() *sql.DB { return s.db }

// Close closes the underlying connection pool. Any registered temp
// file (encryption) must be cleaned up via CloseAt.
func (s *Store) Close() error { return s.db.Close() }

// Open opens (or creates) the SQLite database at path and runs the
// idempotent schema initialisation. Encryption is NOT handled here;
// see OpenEncrypted in encrypt.go for the V1/V2/V3 envelope.
func Open(path string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, fmt.Errorf("eventstore: mkdir: %w", err)
	}
	db, err := sql.Open("sqlite", sqliteDSN(path))
	if err != nil {
		return nil, fmt.Errorf("eventstore: open: %w", err)
	}
	if err := db.Ping(); err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("eventstore: ping: %w", err)
	}
	s := &Store{db: db, path: path}
	if err := s.InitSchema(); err != nil {
		_ = db.Close()
		return nil, err
	}
	return s, nil
}

// sqliteDSN mirrors corekit's sqlitekit DSN: WAL mode, 5s busy
// timeout, foreign_keys on. Keeps the Go port's per-connection
// pragmas identical to db_connection.py.
func sqliteDSN(path string) string {
	return "file:" + path + "?_pragma=busy_timeout(5000)&_pragma=foreign_keys(1)&_pragma=journal_mode(WAL)"
}

// InitSchema creates the campaign / removal_request / request_event /
// request_state tables (and their indexes) idempotently. Sets
// user_version = SchemaVersion on first init; refuses to operate on
// a newer schema.
func (s *Store) InitSchema() error {
	cur, err := s.userVersion()
	if err != nil {
		return fmt.Errorf("eventstore: read user_version: %w", err)
	}
	if cur > SchemaVersion {
		return fmt.Errorf("eventstore: DB at %s has user_version=%d, Go port supports up to %d — refusing to operate",
			s.path, cur, SchemaVersion)
	}
	if cur == SchemaVersion {
		return s.ensureManualTaskSchema()
	}
	stmts := []string{
		`CREATE TABLE IF NOT EXISTS campaigns (
			id              TEXT PRIMARY KEY,
			created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			kind            TEXT NOT NULL DEFAULT 'initial',
			notes           TEXT
		)`,
		`CREATE TABLE IF NOT EXISTS removal_requests (
			id              INTEGER PRIMARY KEY AUTOINCREMENT,
			broker_id       TEXT NOT NULL,
			channel         TEXT NOT NULL DEFAULT 'email',
			campaign_id     TEXT NOT NULL,
			created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			jurisdiction    TEXT NOT NULL,
			template_id     TEXT NOT NULL DEFAULT '',
			identity_snapshot_hash TEXT NOT NULL DEFAULT ''
		)`,
		`CREATE TABLE IF NOT EXISTS request_events (
			id              INTEGER PRIMARY KEY AUTOINCREMENT,
			request_id      INTEGER NOT NULL REFERENCES removal_requests(id),
			occurred_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			recorded_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			event_type      TEXT NOT NULL,
			payload_json    TEXT NOT NULL DEFAULT '{}',
			source          TEXT NOT NULL DEFAULT 'system'
		)`,
		`CREATE INDEX IF NOT EXISTS idx_events_request
			ON request_events(request_id, occurred_at)`,
		`CREATE INDEX IF NOT EXISTS idx_events_occurred_at
			ON request_events(occurred_at DESC)`,
		`CREATE TABLE IF NOT EXISTS request_state (
			request_id      INTEGER PRIMARY KEY REFERENCES removal_requests(id),
			current_status  TEXT NOT NULL DEFAULT 'PLANNED',
			last_event_id   INTEGER NOT NULL DEFAULT 0,
			last_event_at   TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			sent_at         TIMESTAMP,
			acknowledged_at TIMESTAMP,
			resolved_at     TIMESTAMP,
			deadline_at     TIMESTAMP,
			next_action_at  TIMESTAMP,
			reminders_sent  INTEGER NOT NULL DEFAULT 0,
			escalation_level INTEGER NOT NULL DEFAULT 0
		)`,
		`CREATE INDEX IF NOT EXISTS idx_request_state_next_action
			ON request_state(next_action_at, current_status)`,
		`CREATE INDEX IF NOT EXISTS idx_removal_requests_campaign
			ON removal_requests(campaign_id)`,
		`CREATE INDEX IF NOT EXISTS idx_removal_requests_broker
			ON removal_requests(broker_id)`,
		`CREATE INDEX IF NOT EXISTS idx_removal_requests_jurisdiction
			ON removal_requests(jurisdiction)`,
		fmt.Sprintf(`PRAGMA user_version = %d`, SchemaVersion),
	}
	for _, q := range stmts {
		if _, err := s.db.Exec(q); err != nil {
			return fmt.Errorf("eventstore: schema exec %q: %w", firstLine(q), err)
		}
	}
	return s.ensureManualTaskSchema()
}

func (s *Store) ensureManualTaskSchema() error {
	stmts := []string{
		`CREATE TABLE IF NOT EXISTS manual_tasks (
			id                  INTEGER PRIMARY KEY AUTOINCREMENT,
			request_id          INTEGER REFERENCES removal_requests(id),
			broker_id           TEXT NOT NULL DEFAULT '',
			broker_name         TEXT NOT NULL DEFAULT '',
			form_url            TEXT NOT NULL DEFAULT '',
			reason              TEXT NOT NULL DEFAULT 'generic_error',
			instructions        TEXT NOT NULL DEFAULT '',
			screenshot_path     TEXT NOT NULL DEFAULT '',
			html_snapshot_path  TEXT NOT NULL DEFAULT '',
			form_fields_json    TEXT NOT NULL DEFAULT '{}',
			status              TEXT NOT NULL DEFAULT 'pending',
			created_at          TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			completed_at        TIMESTAMP,
			notes               TEXT NOT NULL DEFAULT ''
		)`,
		`CREATE INDEX IF NOT EXISTS idx_manual_tasks_status ON manual_tasks(status)`,
		`CREATE INDEX IF NOT EXISTS idx_manual_tasks_request ON manual_tasks(request_id)`,
	}
	for _, q := range stmts {
		if _, err := s.db.Exec(q); err != nil {
			return fmt.Errorf("eventstore: manual task schema exec %q: %w", firstLine(q), err)
		}
	}
	return nil
}

func firstLine(s string) string {
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return strings.TrimSpace(s[:i])
	}
	return strings.TrimSpace(s)
}

func (s *Store) userVersion() (int, error) {
	var v int
	row := s.db.QueryRow("PRAGMA user_version")
	if err := row.Scan(&v); err != nil {
		return 0, err
	}
	return v, nil
}

// appendTx writes a request_events row inside the given transaction
// and returns the new id. Caller is responsible for committing or
// rolling back the tx.
func appendTx(ctx context.Context, tx *sql.Tx, requestID int64, eventType EventType, payload map[string]any, source Source, occurredAt time.Time) (int64, error) {
	if !eventType.IsValid() {
		return 0, fmt.Errorf("%w: %q", ErrUnknownEventType, eventType)
	}
	if !source.IsValid() {
		return 0, fmt.Errorf("%w: %q", ErrUnknownSource, source)
	}
	if payload == nil {
		payload = map[string]any{}
	}
	pj, err := json.Marshal(payload)
	if err != nil {
		return 0, fmt.Errorf("eventstore: marshal payload: %w", err)
	}
	occurred := timeutil.FormatSQL(occurredAt)
	recorded := timeutil.FormatSQL(time.Now().UTC())
	res, err := tx.ExecContext(ctx,
		`INSERT INTO request_events
		 (request_id, occurred_at, recorded_at, event_type, payload_json, source)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		requestID, occurred, recorded, string(eventType), string(pj), string(source),
	)
	if err != nil {
		return 0, fmt.Errorf("eventstore: INSERT request_events: %w", err)
	}
	return res.LastInsertId()
}

// Append writes a single event and commits. Use AppendAndProject
// when you also want the projection to update atomically.
func (s *Store) Append(
	ctx context.Context,
	requestID int64,
	eventType EventType,
	payload map[string]any,
	source Source,
	occurredAt time.Time,
) (int64, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return 0, err
	}
	id, err := appendTx(ctx, tx, requestID, eventType, payload, source, occurredAt)
	if err != nil {
		_ = tx.Rollback()
		return 0, err
	}
	if err := tx.Commit(); err != nil {
		return 0, err
	}
	return id, nil
}

// AppendAndProject bundles Append + UpsertState in one transaction
// (mirrors projection.append_event_and_project).
func (s *Store) AppendAndProject(
	ctx context.Context,
	requestID int64,
	eventType EventType,
	payload map[string]any,
	source Source,
	occurredAt time.Time,
) (int64, StateJSON, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return 0, StateJSON{}, err
	}
	id, err := appendTx(ctx, tx, requestID, eventType, payload, source, occurredAt)
	if err != nil {
		_ = tx.Rollback()
		return 0, StateJSON{}, err
	}
	state, err := upsertStateTx(ctx, tx, requestID)
	if err != nil {
		_ = tx.Rollback()
		return 0, StateJSON{}, err
	}
	if err := tx.Commit(); err != nil {
		return 0, StateJSON{}, err
	}
	return id, state, nil
}

// CreateCampaign inserts a campaign row, returning false if the id is
// already present.
func (s *Store) CreateCampaign(ctx context.Context, id, kind, notes string) (bool, error) {
	if kind == "" {
		kind = "initial"
	}
	res, err := s.db.ExecContext(ctx,
		`INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?, ?, ?)`,
		id, kind, notes,
	)
	if err != nil {
		return false, err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return false, err
	}
	return n == 1, nil
}

// CreateRemovalRequest inserts a removal_request and returns the new id.
func (s *Store) CreateRemovalRequest(ctx context.Context, brokerID, channel, campaignID, jurisdiction, templateID, hash string) (int64, error) {
	if channel == "" {
		channel = "email"
	}
	res, err := s.db.ExecContext(ctx,
		`INSERT INTO removal_requests
		 (broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		brokerID, channel, campaignID, jurisdiction, templateID, hash,
	)
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}

// GetRemovalRequest fetches one removal_request row.
func (s *Store) GetRemovalRequest(ctx context.Context, id int64) (map[string]any, error) {
	row := s.db.QueryRowContext(ctx,
		`SELECT id, broker_id, channel, campaign_id, created_at, jurisdiction,
		        template_id, identity_snapshot_hash
		 FROM removal_requests WHERE id = ?`, id)
	return scanRequest(row)
}

func scanRequest(row *sql.Row) (map[string]any, error) {
	var (
		id        int64
		brokerID  string
		channel   string
		campaign  string
		createdAt string
		juris     string
		template  string
		hash      string
	)
	if err := row.Scan(&id, &brokerID, &channel, &campaign, &createdAt, &juris, &template, &hash); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil, nil
		}
		return nil, err
	}
	return map[string]any{
		"id":                     id,
		"broker_id":              brokerID,
		"channel":                channel,
		"campaign_id":            campaign,
		"created_at":             createdAt,
		"jurisdiction":           juris,
		"template_id":            template,
		"identity_snapshot_hash": hash,
	}, nil
}

// ListCampaigns returns all campaigns, newest first.
func (s *Store) ListCampaigns(ctx context.Context) ([]map[string]any, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []map[string]any
	for rows.Next() {
		var id, created, kind string
		var notes sql.NullString
		if err := rows.Scan(&id, &created, &kind, &notes); err != nil {
			return nil, err
		}
		out = append(out, map[string]any{
			"id":         id,
			"created_at": created,
			"kind":       kind,
			"notes":      nullStr(notes),
		})
	}
	return out, rows.Err()
}

// GetEvents returns the events for one request, replay-ordered.
func (s *Store) GetEvents(ctx context.Context, requestID int64, afterEventID int64) ([]Event, error) {
	q := `SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
	      FROM request_events
	      WHERE request_id = ?`
	args := []any{requestID}
	if afterEventID > 0 {
		q += ` AND id > ?`
		args = append(args, afterEventID)
	}
	q += ` ORDER BY occurred_at ASC, id ASC`
	rows, err := s.db.QueryContext(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Event
	for rows.Next() {
		var r rawEvent
		if err := rows.Scan(&r.ID, &r.RequestID, &r.OccurredAt, &r.RecordedAt, &r.EventType, &r.Payload, &r.Source); err != nil {
			return nil, err
		}
		ev, err := r.toEvent()
		if err != nil {
			// Skip unparseable rows (forward compatibility).
			slog.Default().Warn("eventstore: skipping unparseable event", "id", r.ID, "err", err)
			continue
		}
		out = append(out, ev)
	}
	return out, rows.Err()
}

// LoadEventsForReplay is the same as GetEvents but tolerant of
// per-row parse failures: it logs and skips rows that fail to
// decode. This is what rebuild_state() uses (mirrors the Python
// `_accumulate_state` try/except).
func (s *Store) LoadEventsForReplay(ctx context.Context, requestID int64) ([]Event, error) {
	return s.GetEvents(ctx, requestID, 0)
}

func nullStr(ns sql.NullString) any {
	if ns.Valid {
		return ns.String
	}
	return nil
}
