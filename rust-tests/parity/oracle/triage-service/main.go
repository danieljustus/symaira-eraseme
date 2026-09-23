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

type output struct {
	SourceSHA256 map[string]string `json:"source_sha256"`
	Classify     map[string]any    `json:"classify"`
	Rebuttal     map[string]any    `json:"rebuttal"`
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
		"classification": classification.Label, "event_type": classification.EventType,
		"confidence": classification.Confidence, "summary": classification.Summary,
		"needs_human_review": classification.NeedsHumanReview, "row_classified_as": rows[0].ClassifiedAs,
		"event_payload": events[0].Payload,
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
		"template_name": rebuttal.TemplateName, "classification": rebuttal.RejectionClassification,
		"confidence": rebuttal.Confidence, "llm_used": rebuttal.LLMUsed,
		"needs_human_review": rebuttal.NeedsHumanReview, "event_type": events[1].EventType,
		"event_payload": events[1].Payload,
	}
	result := output{SourceSHA256: sourceHashes(), Classify: classify, Rebuttal: rebuttalResult}
	encoded, err := json.Marshal(result)
	fatal(err)
	_, _ = os.Stdout.Write(append(encoded, '\n'))
}

func sourceHashes() map[string]string {
	paths := []string{
		"internal/triage/classifier.go", "internal/triage/rebuttal.go",
		"internal/replies/service.go", "internal/replies/repository.go", "internal/llm/llm.go",
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

func fatal(err error) {
	if err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
