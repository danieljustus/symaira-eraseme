package replies

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

func newRepliesTestStore(t *testing.T) (*eventstore.Store, int64) {
	t.Helper()
	store, err := eventstore.Open(t.TempDir() + "/replies.db")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker", "email", "campaign", "GDPR", "", "")
	if err != nil {
		t.Fatal(err)
	}
	return store, requestID
}

func TestRepositoryInsertIsIdempotentAndBoundsSnippets(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	repo := NewRepository(store)
	ctx := context.Background()
	longSnippet := strings.Repeat("x", 2501)
	message := email.Message{ID: "imap-1", MessageID: "<message-1>", ThreadID: "thread", From: "broker@example.test", Subject: "Reply"}
	matched := email.MatchedMessage{Message: message, RequestID: &requestID}
	if err := repo.Insert(ctx, matched, longSnippet); err != nil {
		t.Fatal(err)
	}
	if err := repo.Insert(ctx, matched, "replacement"); err != nil {
		t.Fatal(err)
	}
	rows, err := repo.List(ctx, "unclassified", &requestID)
	if err != nil || len(rows) != 1 {
		t.Fatalf("rows = %#v, err=%v", rows, err)
	}
	if len(rows[0].Snippet) != 2000 || rows[0].MessageID != "<message-1>" {
		t.Fatalf("stored reply = %#v", rows[0])
	}
	if got, err := repo.Get(ctx, rows[0].ID); err != nil || got == nil || got.RequestID == nil || *got.RequestID != requestID {
		t.Fatalf("Get = %#v, err=%v", got, err)
	}
	if got, err := repo.Get(ctx, 999999); err != nil || got != nil {
		t.Fatalf("missing Get = %#v, err=%v", got, err)
	}
}

func TestRepositoryDraftLifecycleAndStatusFilters(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	repo := NewRepository(store)
	ctx := context.Background()
	replyID, err := repo.InsertReply(ctx, &requestID, "message-2", "thread", "broker@example.test", "Rejected", "please verify", "rejected")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := repo.InsertDraft(ctx, replyID, requestID, "draft body", "Re: Rejected", "account"); err != nil {
		t.Fatal(err)
	}
	if draft, err := repo.ExistingDraft(ctx, replyID); err != nil || draft == nil || draft.Body != "draft body" {
		t.Fatalf("ExistingDraft = %#v, err=%v", draft, err)
	}
	if draft, err := repo.LatestDraft(ctx, replyID); err != nil || draft == nil {
		t.Fatalf("LatestDraft = %#v, err=%v", draft, err)
	}
	for _, status := range []string{"needs_reply", "drafted", "classified"} {
		rows, err := repo.List(ctx, status, nil)
		if err != nil || len(rows) != 1 {
			t.Fatalf("status %s rows=%#v err=%v", status, rows, err)
		}
	}
	draft, err := repo.LatestDraft(ctx, replyID)
	if err != nil || draft == nil {
		t.Fatal(err)
	}
	if err := repo.MarkDraftSent(ctx, draft.ID, "account"); err != nil {
		t.Fatal(err)
	}
	rows, err := repo.List(ctx, "sent", nil)
	if err != nil || len(rows) != 1 || rows[0].ClassifierConfidence != nil {
		t.Fatalf("sent rows=%#v err=%v", rows, err)
	}
}

type fakeReplyLLM struct {
	response string
	calls    int
}

func (f *fakeReplyLLM) IsAvailable() bool { return true }
func (f *fakeReplyLLM) Classify(context.Context, string, string, llm.ClassifyOptions) (string, llm.UsageRecord, error) {
	f.calls++
	return f.response, llm.UsageRecord{Model: "fake"}, nil
}
func (f *fakeReplyLLM) Close() error { return nil }

func TestClassifyReplyPersistsClassificationAndEvent(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	ctx := context.Background()
	repo := NewRepository(store)
	if _, err := repo.InsertReply(ctx, &requestID, "message-3", "thread", "broker@example.test", "Acknowledged", "we received your request", ""); err != nil {
		t.Fatal(err)
	}
	fake := &fakeReplyLLM{response: `{"classification":"ack","confidence":0.91,"summary":"received","extracted_fields":{}}`}
	result, err := NewService(store).ClassifyReply(ctx, ClassifyRequest{RequestID: requestID, BrokerName: "Broker", Client: fake, Save: true})
	if err != nil || result.Label != "ack" || fake.calls != 1 {
		t.Fatalf("classification=%#v err=%v calls=%d", result, err, fake.calls)
	}
	classified, err := repo.List(ctx, "classified", &requestID)
	if err != nil || len(classified) != 1 || classified[0].ClassifiedAs != "ack" {
		t.Fatalf("classified=%#v err=%v", classified, err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil || len(events) < 1 || events[len(events)-1].EventType != eventstore.EvtAck {
		t.Fatalf("events=%#v err=%v", events, err)
	}
}

func TestAutoConfirmReturnsNoReplyWithoutSideEffects(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	result, err := NewService(store).AutoConfirm(context.Background(), AutoConfirmRequest{RequestID: requestID})
	if err != nil || result.Success || result.Step != "no_reply" {
		t.Fatalf("result=%#v err=%v", result, err)
	}
	if !strings.Contains(result.Error, fmt.Sprintf("#%d", requestID)) {
		t.Fatalf("error=%q does not name request", result.Error)
	}
}
