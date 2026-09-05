package campaign

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

func TestWebFormNoExecutorPersistsManualFallback(t *testing.T) {
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	broker := registry.Broker{ID: "synthetic-broker", Name: "Synthetic Broker", OptOut: []registry.Channel{{
		Type: "web_form", URL: "https://synthetic.example/optout", FormSpec: &registry.FormSpec{Steps: []registry.FormStep{{Click: "#submit"}}},
	}}}
	adapter := NewWebFormAdapterWithStore(store, []registry.Broker{broker}, nil)
	result := adapter.Run(context.Background(), broker.ID, false)
	if result["success"] != false || result["status"] != "manual_action_required" || result["reason"] != "dynamic_form" || result["dry_run"] != false {
		t.Fatalf("fallback result = %#v", result)
	}
	taskID, ok := result["task_id"].(int64)
	if !ok || taskID == 0 {
		t.Fatalf("task id = %#v", result["task_id"])
	}
	task, err := manualtasks.Get(context.Background(), store, taskID)
	if err != nil || task == nil || task.Status != "pending" || task.BrokerID != broker.ID || task.FormURL != broker.OptOut[0].URL {
		t.Fatalf("task = %#v, err=%v", task, err)
	}
}

func TestWebFormExecutorReceivesBoundedContextAndMapsEvidence(t *testing.T) {
	broker := registry.Broker{ID: "synthetic-broker", Name: "Synthetic Broker", OptOut: []registry.Channel{{
		Type: "web_form", URL: "https://synthetic.example/optout", FormSpec: &registry.FormSpec{TimeoutSeconds: floatPointer(1), Steps: []registry.FormStep{{Fill: map[string]string{"#email": "${email}"}}}},
	}}}
	called := false
	adapter := NewWebFormAdapter([]registry.Broker{broker}, FormExecutorFuncs{Submit: func(ctx context.Context, spec FormSpec) (*Result, error) {
		called = true
		if _, ok := ctx.Deadline(); !ok {
			t.Fatal("executor context has no deadline")
		}
		return &Result{Code: CodeBlockedCaptcha, Message: "synthetic captcha", Evidence: &Evidence{FinalURL: "https://synthetic.example/blocked"}}, nil
	}})
	result := adapter.Run(context.Background(), broker.ID, false)
	if !called || result["success"] != false || result["reason"] != "captcha_failed" || result["url"] != "https://synthetic.example/blocked" {
		t.Fatalf("executor result = %#v called=%v", result, called)
	}
}

func TestExecuteWebFormFallbackOrdersManualAndFailedEvents(t *testing.T) {
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "synthetic-broker", "web_form", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := store.AppendAndProject(ctx, requestID, eventstore.EvtPlanned, map[string]any{"form_url": "https://synthetic.example/optout"}, eventstore.SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	result, err := ExecuteRequest(ctx, store, requestID, ExecuteOpts{})
	if err != nil || result["success"] != false || result["task_id"] == nil {
		t.Fatalf("fallback result=%#v err=%v", result, err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	var human, failed int
	for i, event := range events {
		switch event.EventType {
		case eventstore.EvtHumanActionRequired:
			human = i
		case eventstore.EvtSendFailed:
			failed = i
		}
	}
	if human == 0 || failed <= human {
		t.Fatalf("event order=%#v", events)
	}
}
