// Command triage-service runs the production Go triage/service path with a
// deterministic injected client and emits only persisted, non-time fields.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

type fakeClient struct{}

func (fakeClient) IsAvailable() bool { return true }
func (fakeClient) Close() error      { return nil }
func (fakeClient) Classify(_ context.Context, system, _ string, _ llm.ClassifyOptions) (string, llm.UsageRecord, error) {
	if strings.Contains(system, "rejection classifier") {
		return `{"classification":"address_mismatch","confidence":0.9,"summary":"address","jurisdiction":"GDPR","key_points":["old address"]}`, llm.UsageRecord{Model: "fake"}, nil
	}
	return `{"classification":"ack","confidence":0.91,"summary":"received","extracted_fields":{"case":"42"}}`, llm.UsageRecord{Model: "fake"}, nil
}

type failingClient struct{}

func (failingClient) IsAvailable() bool { return true }
func (failingClient) Close() error      { return nil }
func (failingClient) Classify(context.Context, string, string, llm.ClassifyOptions) (string, llm.UsageRecord, error) {
	return "", llm.UsageRecord{}, fmt.Errorf("fake classifier failure")
}

type output struct {
	SourceSHA256 map[string]string `json:"source_sha256"`
	Classify     map[string]any    `json:"classify"`
	Rebuttal     map[string]any    `json:"rebuttal"`
	Fallback     map[string]any    `json:"fallback"`
	LlmError     map[string]any    `json:"llm_error"`
}

func main() {
	ctx := context.Background()
	tmp, err := os.MkdirTemp("", "dom004-triage-service-")
	fatal(err)
	defer os.RemoveAll(tmp)
	store, err := eventstore.Open(filepath.Join(tmp, "oracle.db"))
	fatal(err)
	defer store.Close()
	requestID, err := store.CreateRemovalRequest(ctx, "broker", "email", "campaign", "GDPR", "template", "hash")
	fatal(err)
	repo := replies.NewRepository(store)
	replyID, err := repo.InsertReply(ctx, &requestID, "message-1", "thread", "broker@example.test", "Reply subject", "We received your request", "")
	fatal(err)
	if replyID != 1 {
		fatal(fmt.Errorf("unexpected reply id %d", replyID))
	}
	client := fakeClient{}
	service := replies.NewService(store)
	classification, err := service.ClassifyReply(ctx, replies.ClassifyRequest{RequestID: requestID, BrokerName: "Example", Client: client, Save: true})
	fatal(err)
	rows, err := repo.List(ctx, "classified", &requestID)
	fatal(err)
	events, err := store.GetEvents(ctx, requestID, 0)
	fatal(err)
	if len(rows) != 1 || len(events) != 1 {
		fatal(fmt.Errorf("unexpected persisted rows/events: %d/%d", len(rows), len(events)))
	}
	classify := map[string]any{
		"result": map[string]any{
			"classification": classification.Label, "event_type": classification.EventType,
			"confidence": classification.Confidence, "summary": classification.Summary,
			"extracted_fields":   classification.ExtractedFields,
			"needs_human_review": classification.NeedsHumanReview,
			"usage":              classification.Usage.Record(),
		},
		"reply": map[string]any{
			"id": rows[0].ID, "request_id": rows[0].RequestID, "message_id": rows[0].MessageID,
			"thread_id": rows[0].ThreadID, "from": rows[0].From, "subject": rows[0].Subject,
			"snippet": rows[0].Snippet, "classified_as": rows[0].ClassifiedAs,
			"classifier_confidence": rows[0].ClassifierConfidence, "llm_summary": rows[0].LLMSummary,
		},
		"events":     eventRecords(events),
		"projection": requestProjection(ctx, store, requestID),
	}
	rebuttal, err := service.GenerateRebuttal(ctx, replies.RebuttalRequest{
		RequestID: requestID, BrokerName: "Example", OriginalRequestTemplate: "original request",
		OriginalRequestDate: "2026-09-01", Client: client, Save: true,
	})
	fatal(err)
	events, err = store.GetEvents(ctx, requestID, 0)
	fatal(err)
	if len(events) != 2 {
		fatal(fmt.Errorf("expected two persisted events, got %d", len(events)))
	}
	rebuttalResult := map[string]any{
		"result": map[string]any{
			"template_name": rebuttal.TemplateName, "label": rebuttal.Label,
			"description": rebuttal.Description, "jurisdiction": rebuttal.Jurisdiction,
			"rejection_classification": rebuttal.RejectionClassification,
			"confidence":               rebuttal.Confidence, "rebuttal_body": rebuttal.RebuttalBody,
			"needs_human_review": rebuttal.NeedsHumanReview, "llm_used": rebuttal.LLMUsed,
			"usage": rebuttal.Usage.Record(),
		},
		"events":     eventRecords(events),
		"projection": requestProjection(ctx, store, requestID),
	}
	result := output{SourceSHA256: sourceHashes(), Classify: classify, Rebuttal: rebuttalResult}
	result.Fallback = runRebuttalScenario(ctx, store, "fallback", nil)
	result.LlmError = runRebuttalScenario(ctx, store, "llm-error", failingClient{})
	encoded, err := json.Marshal(result)
	fatal(err)
	_, _ = os.Stdout.Write(append(encoded, '\n'))
}

func runRebuttalScenario(ctx context.Context, store *eventstore.Store, campaign string, client llm.Client) map[string]any {
	requestID, err := store.CreateRemovalRequest(ctx, "broker-"+campaign, "email", campaign, "GDPR", "template", "hash")
	fatal(err)
	result, err := replies.NewService(store).GenerateRebuttal(ctx, replies.RebuttalRequest{
		RequestID: requestID, BrokerName: "Example", OriginalRequestTemplate: "The old address on file is wrong",
		OriginalRequestDate: "2026-09-01", Client: client, Save: true,
	})
	fatal(err)
	events, err := store.GetEvents(ctx, requestID, 0)
	fatal(err)
	if len(events) != 1 {
		fatal(fmt.Errorf("expected one fallback event, got %d", len(events)))
	}
	return map[string]any{
		"result": map[string]any{
			"template_name": result.TemplateName, "label": result.Label, "description": result.Description,
			"jurisdiction": result.Jurisdiction, "rejection_classification": result.RejectionClassification,
			"confidence": result.Confidence, "rebuttal_body": result.RebuttalBody,
			"needs_human_review": result.NeedsHumanReview, "llm_used": result.LLMUsed,
			"usage": result.Usage.Record(),
		},
		"events":     eventRecords(events),
		"projection": requestProjection(ctx, store, requestID),
	}
}

func requestProjection(ctx context.Context, store *eventstore.Store, requestID int64) map[string]any {
	var status string
	var lastEventID, remindersSent, escalationLevel int64
	err := store.DB().QueryRowContext(ctx, `SELECT current_status, last_event_id, reminders_sent, escalation_level
		FROM request_state WHERE request_id = ?`, requestID).Scan(&status, &lastEventID, &remindersSent, &escalationLevel)
	fatal(err)
	return map[string]any{
		"current_status": status, "last_event_id": lastEventID,
		"reminders_sent": remindersSent, "escalation_level": escalationLevel,
	}
}

func sourceHashes() map[string]string {
	paths := []string{
		"internal/triage/classifier.go", "internal/triage/rebuttal.go",
		"internal/replies/service.go", "internal/replies/repository.go", "internal/llm/llm.go",
		"internal/eventstore/store.go", "internal/eventstore/projection.go",
		"rust-tests/parity/oracle/triage-service/main.go",
	}
	result := make(map[string]string, len(paths))
	for _, name := range paths {
		contents, err := os.ReadFile(name)
		fatal(err)
		digest := sha256.Sum256(contents)
		result[name] = hex.EncodeToString(digest[:])
	}
	return result
}

func eventRecords(events []eventstore.Event) []map[string]any {
	result := make([]map[string]any, 0, len(events))
	for _, event := range events {
		result = append(result, map[string]any{
			"id": event.ID, "request_id": event.RequestID,
			"event_type": event.EventType, "source": event.Source,
			"payload": event.Payload,
		})
	}
	return result
}

func fatal(err error) {
	if err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
