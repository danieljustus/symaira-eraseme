package manualtasks

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestServiceHandlersCoverListShowCompleteAndMissingTasks(t *testing.T) {
	store, requestID := testStore(t)
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	ctx := context.Background()
	task, err := Create(ctx, store, CreateOpts{
		RequestID:         &requestID,
		BrokerID:          "broker-id",
		BrokerName:        "Broker Name",
		FormURL:           "https://example.test/form",
		Reason:            "timeout",
		ExtraInstructions: "Bring the request reference.",
	})
	if err != nil {
		t.Fatal(err)
	}

	listed, err := HandleList(ctx, store, ListOpts{})
	if err != nil || !listed.Success || !strings.Contains(listed.Data["message"].(string), "Broker Name") {
		t.Fatalf("HandleList = %#v, err=%v", listed, err)
	}
	shown, err := HandleShow(ctx, store, task.ID)
	if err != nil || !shown.Success || !strings.Contains(shown.Data["message"].(string), "Bring the request reference.") {
		t.Fatalf("HandleShow = %#v, err=%v", shown, err)
	}
	completed, err := HandleComplete(ctx, store, task.ID, "done")
	if err != nil || !completed.Success || completed.Data["task_id"] != task.ID {
		t.Fatalf("HandleComplete = %#v, err=%v", completed, err)
	}

	missingShow, err := HandleShow(ctx, store, 999999)
	if err != nil || missingShow.Success || !strings.Contains(missingShow.Error, "not found") {
		t.Fatalf("missing HandleShow = %#v, err=%v", missingShow, err)
	}
	missingComplete, err := HandleComplete(ctx, store, 999999, "none")
	if err != nil || missingComplete.Success || !strings.Contains(missingComplete.Error, "not found") {
		t.Fatalf("missing HandleComplete = %#v, err=%v", missingComplete, err)
	}
}

func TestServiceCleanupUsesConfiguredDataDirectory(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	tasksDir := filepath.Join(dataDir, "manual_tasks")
	if err := os.MkdirAll(tasksDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tasksDir, "snapshot.html"), []byte("html"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tasksDir, "keep.txt"), []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}

	preview, err := HandleCleanup(true)
	if err != nil || !preview.Success || preview.Data["skipped"] != 1 {
		t.Fatalf("cleanup preview = %#v, err=%v", preview, err)
	}
	result, err := HandleCleanup(false)
	if err != nil || !result.Success || result.Data["removed"] != 1 {
		t.Fatalf("cleanup result = %#v, err=%v", result, err)
	}
	if _, err := os.Stat(filepath.Join(tasksDir, "keep.txt")); err != nil {
		t.Fatal("cleanup removed unrelated file")
	}
}

func TestResultMarshalJSONOmitsMessageWhenErrorIsPresent(t *testing.T) {
	encoded, err := json.Marshal(Result{Success: false, Error: "failed", Data: map[string]any{"message": "internal", "task_id": 4}})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(encoded), "internal") || !strings.Contains(string(encoded), "failed") {
		t.Fatalf("error payload leaked message or omitted error: %s", encoded)
	}
}
