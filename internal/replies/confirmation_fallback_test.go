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
	if _, err := repo.InsertReply(ctx, &requestID, "manual-confirm", "thread", "broker@custom.example", "Confirm", "Confirm here: https://custom.example/confirm", ""); err != nil {
		t.Fatal(err)
	}
	result, err := NewService(store).AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID})
	if err != nil {
		t.Fatal(err)
	}
	if result.Success || !result.ManualActionRequired || result.Step != "manual_confirmation_required" || result.Status != "manual_action_required" || result.TaskID == 0 {
		t.Fatalf("manual confirmation result = %#v", result)
	}
	if result.ClickedURL != "https://custom.example/confirm" || result.Reason != "dynamic_form" || result.Instructions == "" {
		t.Fatalf("manual confirmation details = %#v", result)
	}
	task, err := manualtasks.Get(ctx, store, result.TaskID)
	if err != nil || task == nil || task.Status != "pending" || task.FormURL != result.ClickedURL || task.RequestID == nil || *task.RequestID != requestID {
		t.Fatalf("manual task = %#v, err=%v", task, err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) < 2 || events[len(events)-2].EventType != eventstore.EvtHumanActionRequired || events[len(events)-1].EventType != eventstore.EvtNoteAdded {
		t.Fatalf("manual fallback event order = %#v", events)
	}
	for _, event := range events {
		if event.EventType == eventstore.EvtConfirmationLinkClicked {
			t.Fatalf("manual fallback claimed click: %#v", events)
		}
	}
}
