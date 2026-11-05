package replies

import (
	"context"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
)

func TestAutoConfirmCreatesManualConfirmationTaskWithoutClick(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	ctx := context.Background()
	repo := NewRepository(store)
	if _, err := repo.InsertReply(ctx, &requestID, "manual-confirm", "thread", "broker@acxiom.com", "Confirm", "Confirm here: https://acxiom.com/confirm", ""); err != nil {
		t.Fatal(err)
	}
	result, err := NewService(store).AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID})
	if err != nil {
		t.Fatal(err)
	}
	if result.Success || !result.ManualActionRequired || result.Step != "manual_confirmation_required" || result.Status != "manual_action_required" || result.TaskID == 0 {
		t.Fatalf("manual confirmation result = %#v", result)
	}
	if result.ClickedURL != "https://acxiom.com/confirm" || result.Reason != "dynamic_form" || result.Instructions == "" {
		t.Fatalf("manual confirmation details = %#v", result)
	}
	task, err := manualtasks.Get(ctx, store, result.TaskID)
	if err != nil || task == nil || task.Status != "pending" || task.FormURL != result.ClickedURL || task.RequestID == nil || *task.RequestID != requestID {
		t.Fatalf("manual task = %#v, err=%v", task, err)
	}
	repeated, err := NewService(store).AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID})
	if err != nil || repeated.TaskID != result.TaskID {
		t.Fatalf("repeated fallback=%#v err=%v", repeated, err)
	}
	tasks, err := manualtasks.List(ctx, store, manualtasks.ListOpts{RequestID: &requestID})
	if err != nil || len(tasks) != 1 {
		t.Fatalf("duplicate confirmation tasks=%#v err=%v", tasks, err)
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
		if event.EventType == eventstore.EvtNoteAdded {
			t.Fatalf("expected manual fallback created a failure note: %#v", events)
		}
		if event.EventType == eventstore.EvtConfirmationLinkClicked {
			t.Fatalf("manual fallback claimed click: %#v", events)
		}
	}
	if humanEvents != 1 {
		t.Fatalf("human-action events=%d, want 1: %#v", humanEvents, events)
	}
}
