package replies

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func TestRepositoryAndAutoConfirmService(t *testing.T) {
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "test-broker", "email", "campaign", "GDPR", "gdpr-art17.en.md.j2", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := store.AppendAndProject(ctx, requestID, eventstore.EvtPlanned, nil, eventstore.SrcSystem, nowUTC()); err != nil {
		t.Fatal(err)
	}
	repo := NewRepository(store)
	if _, err := repo.InsertReply(ctx, &requestID, "message-1", "thread-1", "broker@acxiom.com", "Reply", "Click https://acxiom.com/confirm", ""); err != nil {
		t.Fatal(err)
	}
	service := NewService(store)
	result, err := service.AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID, DryRun: true})
	if err != nil || !result.Success || result.ClickedURL != "https://acxiom.com/confirm" {
		t.Fatalf("result=%#v err=%v", result, err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("dry run added event: %#v", events)
	}
}
