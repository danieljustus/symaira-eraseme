package campaign

import (
	"context"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
)

func TestFailedWebFormCreatesManualTask(t *testing.T) {
	store, result, profilePath := setupPlan(t)
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	var requestID int64
	for _, request := range result.Requests {
		if request.Channel == "web_form" {
			requestID = request.RequestID
			break
		}
	}
	if requestID == 0 {
		t.Fatal("no web-form request planned")
	}
	failure := func(context.Context, string, bool) map[string]any {
		return map[string]any{
			"success": false, "error": "captcha unsolved", "url": "https://form.example/optout",
			"reason": "captcha_failed", "step_index": float64(2), "total_steps": float64(5),
			"screenshot_path": "/tmp/form.png", "html_snapshot": "<html>jane@example.com</html>",
			"form_fields": map[string]any{"email": "jane@example.com"},
		}
	}
	out, err := ExecuteRequest(context.Background(), store, requestID, ExecuteOpts{
		WebForm: failure, ProfilePath: profilePath,
	})
	if err != nil {
		t.Fatal(err)
	}
	taskID, ok := out["task_id"].(int64)
	if !ok || taskID == 0 {
		t.Fatalf("task_id = %#v", out["task_id"])
	}
	task, err := manualtasks.Get(context.Background(), store, taskID)
	if err != nil || task == nil {
		t.Fatalf("manual task = %#v, err=%v", task, err)
	}
	if task.Reason != "captcha_failed" || task.Status != "pending" || task.FormURL != "https://form.example/optout" {
		t.Fatalf("task = %#v", task)
	}
	events, err := store.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) < 2 || string(events[len(events)-2].EventType) != "HUMAN_ACTION_REQUIRED" || string(events[len(events)-1].EventType) != "SEND_FAILED" {
		t.Fatalf("events = %#v", events)
	}
	if got, _ := events[len(events)-1].Payload["task_id"].(float64); int64(got) != taskID {
		// JSON-decoded values are float64; direct event values are int64.
		if direct, ok := events[len(events)-1].Payload["task_id"].(int64); !ok || direct != taskID {
			t.Fatalf("SEND_FAILED task_id = %#v", events[len(events)-1].Payload["task_id"])
		}
	}
}
