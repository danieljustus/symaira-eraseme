package manualtasks

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func testStore(t *testing.T) (*eventstore.Store, int64) {
	t.Helper()
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "symeraseme.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	requestID, err := store.CreateRemovalRequest(context.Background(), "test-broker", "web_form", "test", "CCPA", "", "")
	if err != nil {
		t.Fatal(err)
	}
	return store, requestID
}

func TestCreatePersistsRedactedTaskAndHumanAction(t *testing.T) {
	store, requestID := testStore(t)
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)

	task, err := Create(context.Background(), store, CreateOpts{
		RequestID: &requestID, BrokerID: "test-broker", BrokerName: "Test Broker",
		FormURL: "https://example.test/optout", Reason: "made_up_reason",
		HTMLSnapshot: "<html>Contact jane@example.com at 555-123-4567</html>",
		FormFields:   map[string]string{"email": "jane@example.com"}, StepIndex: 2,
		TotalSteps: 5, ErrorMessage: "Timeout waiting for selector",
	})
	if err != nil {
		t.Fatal(err)
	}
	if task.Reason != "generic_error" || task.Status != "pending" {
		t.Fatalf("task normalization: %#v", task)
	}
	if task.HTMLSnapshotPath == "" {
		t.Fatal("expected HTML snapshot path")
	}
	contents, err := os.ReadFile(task.HTMLSnapshotPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(contents) == "" || strings.Contains(string(contents), "jane@example.com") || strings.Contains(string(contents), "555-123-4567") {
		t.Fatalf("snapshot was not redacted: %q", contents)
	}
	if mode := fileMode(t, task.HTMLSnapshotPath); mode != 0o600 {
		t.Fatalf("snapshot mode = %o, want 600", mode)
	}
	stored, err := Get(context.Background(), store, task.ID)
	if err != nil || stored == nil {
		t.Fatalf("get stored task: %v %#v", err, stored)
	}
	if stored.RequestID == nil || *stored.RequestID != requestID {
		t.Fatalf("request id: %#v", stored.RequestID)
	}
	events, err := store.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].EventType != eventstore.EvtHumanActionRequired {
		t.Fatalf("events: %#v", events)
	}
	if events[0].Payload["manual_task_id"] != float64(task.ID) && events[0].Payload["manual_task_id"] != task.ID {
		t.Fatalf("event task id: %#v", events[0].Payload["manual_task_id"])
	}
	state, err := store.RebuildState(context.Background(), requestID)
	if err != nil {
		t.Fatal(err)
	}
	if state.CurrentStatus != "AWAITING_USER_ACTION" {
		t.Fatalf("state = %s", state.CurrentStatus)
	}
}

func TestListFiltersAndCompleteAppendsNote(t *testing.T) {
	store, requestID := testStore(t)
	first, err := Create(context.Background(), store, CreateOpts{RequestID: &requestID, BrokerID: "a", Reason: "timeout"})
	if err != nil {
		t.Fatal(err)
	}
	second, err := Create(context.Background(), store, CreateOpts{BrokerID: "b", Reason: "captcha_failed"})
	if err != nil {
		t.Fatal(err)
	}
	status := "pending"
	filtered, err := List(context.Background(), store, ListOpts{Status: &status, RequestID: &requestID})
	if err != nil || len(filtered) != 1 || filtered[0].ID != first.ID {
		t.Fatalf("filtered = %#v, err=%v", filtered, err)
	}
	completed, err := Complete(context.Background(), store, first.ID, "completed manually", true)
	if err != nil || completed == nil || completed.Status != "completed" || completed.CompletedAt == nil {
		t.Fatalf("completed = %#v, err=%v", completed, err)
	}
	notes, err := store.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if notes[len(notes)-1].EventType != eventstore.EvtNoteAdded || notes[len(notes)-1].Source != eventstore.SrcUser {
		t.Fatalf("note event = %#v", notes[len(notes)-1])
	}
	if second.ID == first.ID {
		t.Fatal("task IDs were not distinct")
	}
}

func TestCleanupDryRunAndApply(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{"snap.png", "page.html", "task.json", "keep.txt"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(name), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	preview, err := Cleanup(dir, true)
	if err != nil || preview.Skipped != 3 || preview.Removed != 0 || !preview.DryRun {
		t.Fatalf("preview = %#v, err=%v", preview, err)
	}
	result, err := Cleanup(dir, false)
	if err != nil || result.Removed != 3 {
		t.Fatalf("result = %#v, err=%v", result, err)
	}
	if _, err := os.Stat(filepath.Join(dir, "keep.txt")); err != nil {
		t.Fatal("cleanup removed non-artifact")
	}
}

func TestToolDefinitionsMatchCapturedRequests(t *testing.T) {
	root := repoRoot(t)
	want := map[string]bool{
		"manual_tasks_list": true, "manual_tasks_show": true,
		"manual_tasks_complete": true, "manual_tasks_cleanup": true,
	}
	defs := ToolDefinitions()
	if len(defs) != len(want) {
		t.Fatalf("definitions = %d, want %d", len(defs), len(want))
	}
	for _, def := range defs {
		name, _ := def["name"].(string)
		if !want[name] {
			t.Fatalf("unexpected definition %q", name)
		}
		fixture, err := os.ReadFile(filepath.Join(root, "tests", "fixtures", "mcp-contract", "requests", name+".request.json"))
		if err != nil {
			t.Fatal(err)
		}
		var request struct {
			Params struct {
				Name      string         `json:"name"`
				Arguments map[string]any `json:"arguments"`
			} `json:"params"`
		}
		if err := json.Unmarshal(fixture, &request); err != nil {
			t.Fatal(err)
		}
		if request.Params.Name != name || request.Params.Arguments == nil {
			t.Fatalf("fixture for %s: %#v", name, request)
		}
	}
}

func TestServiceResultJSONFlattensCliResultData(t *testing.T) {
	encoded, err := json.Marshal(Result{Success: true, Data: map[string]any{
		"message": "Manual tasks (1):", "tasks": []any{map[string]any{"id": 7}},
	}})
	if err != nil {
		t.Fatal(err)
	}
	var payload map[string]any
	if err := json.Unmarshal(encoded, &payload); err != nil {
		t.Fatal(err)
	}
	if payload["success"] != true || payload["message"] != "Manual tasks (1):" {
		t.Fatalf("payload = %#v", payload)
	}
	if _, nested := payload["data"]; nested {
		t.Fatalf("CliResult data must be flattened: %#v", payload)
	}
}

func repoRoot(t *testing.T) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}

func fileMode(t *testing.T, path string) os.FileMode {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	return info.Mode().Perm()
}
