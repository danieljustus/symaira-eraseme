package eventstore_test

import (
	"context"
	"database/sql"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func TestIMAPStatePersistence(t *testing.T) {
	ctx := context.Background()
	dbPath := filepath.Join(t.TempDir(), "symeraseme.db")

	store, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}

	// 1. Non-existent returns nil, nil, nil
	v, u, err := store.GetIMAPHWM(ctx, "imap.example.com", "INBOX")
	if err != nil {
		t.Fatalf("GetIMAPHWM failed: %v", err)
	}
	if v != nil || u != nil {
		t.Fatalf("expected nil for non-existent record, got v=%v, u=%v", v, u)
	}

	// 2. Set HWM
	if err := store.SetIMAPHWM(ctx, "imap.example.com", "INBOX", 42, 100); err != nil {
		t.Fatalf("SetIMAPHWM failed: %v", err)
	}

	// 3. Get HWM
	v, u, err = store.GetIMAPHWM(ctx, "imap.example.com", "INBOX")
	if err != nil {
		t.Fatalf("GetIMAPHWM failed: %v", err)
	}
	if v == nil || *v != 42 || u == nil || *u != 100 {
		t.Fatalf("unexpected HWM: v=%v, u=%v", v, u)
	}

	// 4. Update HWM (UPSERT)
	if err := store.SetIMAPHWM(ctx, "imap.example.com", "INBOX", 42, 150); err != nil {
		t.Fatalf("SetIMAPHWM update failed: %v", err)
	}
	v, u, err = store.GetIMAPHWM(ctx, "imap.example.com", "INBOX")
	if err != nil {
		t.Fatalf("GetIMAPHWM after update failed: %v", err)
	}
	if v == nil || *v != 42 || u == nil || *u != 150 {
		t.Fatalf("unexpected HWM after update: v=%v, u=%v", v, u)
	}

	// Close first store instance
	if err := store.Close(); err != nil {
		t.Fatalf("Close failed: %v", err)
	}

	// 5. Open new store instance on the same file (process/service restart)
	store2, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open store2 failed: %v", err)
	}
	defer store2.Close()

	v, u, err = store2.GetIMAPHWM(ctx, "imap.example.com", "INBOX")
	if err != nil {
		t.Fatalf("store2 GetIMAPHWM failed: %v", err)
	}
	if v == nil || *v != 42 || u == nil || *u != 150 {
		t.Fatalf("store2 unexpected HWM across process restart: v=%v, u=%v", v, u)
	}

	ver, err := store2.UserVersion()
	if err != nil {
		t.Fatalf("UserVersion failed: %v", err)
	}
	if ver != eventstore.SchemaVersion {
		t.Fatalf("UserVersion = %d, want %d", ver, eventstore.SchemaVersion)
	}
}

func TestBackwardMigrationFromV1ToV2(t *testing.T) {
	ctx := context.Background()
	dbPath := filepath.Join(t.TempDir(), "v1.db")

	// 1. Create a database directly with user_version = 1 and v1 tables only
	rawDB, err := sql.Open("sqlite", "file:"+dbPath)
	if err != nil {
		t.Fatalf("sql.Open: %v", err)
	}
	v1Stmts := []string{
		`CREATE TABLE campaigns (id TEXT PRIMARY KEY, created_at TIMESTAMP DEFAULT (datetime('now')), kind TEXT DEFAULT 'initial', notes TEXT)`,
		`CREATE TABLE removal_requests (id INTEGER PRIMARY KEY AUTOINCREMENT, broker_id TEXT NOT NULL, channel TEXT DEFAULT 'email', campaign_id TEXT NOT NULL, created_at TIMESTAMP DEFAULT (datetime('now')), jurisdiction TEXT NOT NULL, template_id TEXT DEFAULT '', identity_snapshot_hash TEXT DEFAULT '')`,
		`CREATE TABLE request_events (id INTEGER PRIMARY KEY AUTOINCREMENT, request_id INTEGER NOT NULL, occurred_at TIMESTAMP DEFAULT (datetime('now')), recorded_at TIMESTAMP DEFAULT (datetime('now')), event_type TEXT NOT NULL, payload_json TEXT DEFAULT '{}', source TEXT DEFAULT 'system')`,
		`CREATE TABLE request_state (request_id INTEGER PRIMARY KEY, current_status TEXT DEFAULT 'PLANNED', last_event_id INTEGER DEFAULT 0, last_event_at TIMESTAMP DEFAULT (datetime('now')), reminders_sent INTEGER DEFAULT 0, escalation_level INTEGER DEFAULT 0)`,
		`INSERT INTO campaigns (id, kind) VALUES ('c1', 'initial')`,
		`INSERT INTO removal_requests (id, broker_id, campaign_id, jurisdiction) VALUES (1, 'acme', 'c1', 'gdpr')`,
		`PRAGMA user_version = 1`,
	}
	for _, q := range v1Stmts {
		if _, err := rawDB.Exec(q); err != nil {
			t.Fatalf("exec v1 stmt %q: %v", q, err)
		}
	}
	// Verify imap_state does not exist yet
	var count int
	err = rawDB.QueryRow("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='imap_state'").Scan(&count)
	if err != nil || count != 0 {
		t.Fatalf("expected imap_state to not exist in v1, count=%d, err=%v", count, err)
	}
	_ = rawDB.Close()

	// 2. Open via eventstore.Open (triggering migration from v1 to v2)
	store, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open v1 database: %v", err)
	}
	defer store.Close()

	// 3. Verify user_version was bumped to 2
	ver, err := store.UserVersion()
	if err != nil {
		t.Fatalf("store.UserVersion: %v", err)
	}
	if ver != 2 {
		t.Fatalf("migrated user_version = %d, want 2", ver)
	}

	// 4. Verify existing campaign and removal request data survived intact
	repo := eventstore.NewRepository(store)
	req, err := repo.GetRemovalRequest(ctx, 1)
	if err != nil || req == nil {
		t.Fatalf("Get request 1 failed: %v, req=%v", err, req)
	}
	if req["broker_id"] != "acme" {
		t.Fatalf("broker_id = %v, want %q", req["broker_id"], "acme")
	}

	// 5. Verify imap_state now works via Get/Set and GetIMAPHWM/SetIMAPHWM
	v, u, err := store.Get(ctx, "imap.migrated.com", "INBOX")
	if err != nil || v != nil || u != nil {
		t.Fatalf("initial Get failed: err=%v, v=%v, u=%v", err, v, u)
	}
	if err := store.Set(ctx, "imap.migrated.com", "INBOX", 99, 1000); err != nil {
		t.Fatalf("Set failed: %v", err)
	}
	v, u, err = store.Get(ctx, "imap.migrated.com", "INBOX")
	if err != nil || v == nil || *v != 99 || u == nil || *u != 1000 {
		t.Fatalf("Get after Set failed: err=%v, v=%v, u=%v", err, v, u)
	}
}

func TestStoreRefusesNewerSchemaVersion(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "future.db")
	rawDB, err := sql.Open("sqlite", "file:"+dbPath)
	if err != nil {
		t.Fatalf("sql.Open: %v", err)
	}
	if _, err := rawDB.Exec("PRAGMA user_version = 999"); err != nil {
		t.Fatalf("set future user_version: %v", err)
	}
	_ = rawDB.Close()

	_, err = eventstore.Open(dbPath)
	if err == nil || !strings.Contains(err.Error(), "refusing to operate") {
		t.Fatalf("expected refusal on future schema, got: %v", err)
	}
}
