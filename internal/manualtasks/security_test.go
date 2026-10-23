package manualtasks

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func TestCreateRedactsIdentityAndDeduplicatesPendingFallback(t *testing.T) {
	store, requestID := manualTaskSecurityStore(t)
	ctx := context.Background()
	profile := &identity.Profile{FullName: "Jane Secret", EmailAddresses: []string{"jane@example.test"}}
	opts := CreateOpts{
		RequestID: &requestID, BrokerID: "broker", BrokerName: "Broker",
		FormURL: "https://broker.test/form?email=jane@example.test", Reason: "dynamic_form",
		HTMLSnapshot: "Jane Secret jane@example.test", FormFields: map[string]string{"email": "jane@example.test"},
		ErrorMessage: "failed for Jane Secret", Profile: profile,
	}
	first, err := Create(ctx, store, opts)
	if err != nil {
		t.Fatal(err)
	}
	second, err := Create(ctx, store, opts)
	if err != nil {
		t.Fatal(err)
	}
	if first.ID != second.ID {
		t.Fatalf("duplicate pending tasks: first=%d second=%d", first.ID, second.ID)
	}
	if strings.Contains(first.FormURL+first.FormFieldsJSON, "jane@example.test") {
		t.Fatalf("identity leaked into task: %#v", first)
	}
	if first.HTMLSnapshotPath == "" {
		t.Fatal("redacted HTML snapshot was not persisted")
	}
	html, err := os.ReadFile(first.HTMLSnapshotPath)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(html), "Jane Secret") || strings.Contains(string(html), "jane@example.test") {
		t.Fatalf("identity leaked into HTML snapshot: %q", html)
	}
	tasks, err := List(ctx, store, ListOpts{})
	if err != nil || len(tasks) != 1 {
		t.Fatalf("tasks=%#v err=%v", tasks, err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	humanEvents := 0
	for _, event := range events {
		if event.EventType == eventstore.EvtHumanActionRequired {
			humanEvents++
		}
	}
	if humanEvents != 1 {
		t.Fatalf("human-action events=%d, want 1", humanEvents)
	}
}

func TestCreateRollsBackTaskWhenEventAppendFails(t *testing.T) {
	store, requestID := manualTaskSecurityStore(t)
	ctx := context.Background()
	if _, err := store.DB().Exec(`CREATE TRIGGER fail_human_event BEFORE INSERT ON request_events
		WHEN NEW.event_type = 'HUMAN_ACTION_REQUIRED' BEGIN SELECT RAISE(ABORT, 'injected event failure'); END`); err != nil {
		t.Fatal(err)
	}
	_, err := Create(ctx, store, CreateOpts{RequestID: &requestID, BrokerID: "broker", BrokerName: "Broker", FormURL: "https://broker.test/form", Reason: "dynamic_form"})
	if err == nil {
		t.Fatal("event failure did not fail task creation")
	}
	tasks, listErr := List(ctx, store, ListOpts{})
	if listErr != nil || len(tasks) != 0 {
		t.Fatalf("rolled-back tasks=%#v err=%v", tasks, listErr)
	}
}

func TestCompleteRollsBackStatusWhenNoteAppendFails(t *testing.T) {
	store, requestID := manualTaskSecurityStore(t)
	ctx := context.Background()
	task, err := Create(ctx, store, CreateOpts{RequestID: &requestID, BrokerID: "broker", BrokerName: "Broker", FormURL: "https://broker.test/form", Reason: "dynamic_form"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.DB().Exec(`CREATE TRIGGER fail_note_event BEFORE INSERT ON request_events
		WHEN NEW.event_type = 'NOTE_ADDED' BEGIN SELECT RAISE(ABORT, 'injected note failure'); END`); err != nil {
		t.Fatal(err)
	}
	if _, err := Complete(ctx, store, task.ID, "done", true); err == nil {
		t.Fatal("note failure did not fail completion")
	}
	persisted, err := Get(ctx, store, task.ID)
	if err != nil || persisted == nil || persisted.Status != "pending" || persisted.CompletedAt != nil {
		t.Fatalf("task after rollback=%#v err=%v", persisted, err)
	}
}

func manualTaskSecurityStore(t *testing.T) (*eventstore.Store, int64) {
	t.Helper()
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker", "web_form", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	return store, requestID
}
