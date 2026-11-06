package triage

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type fakeClient struct {
	response string
	err      error
}

func (f fakeClient) IsAvailable() bool { return true }
func (f fakeClient) Classify(context.Context, string, string, llm.ClassifyOptions) (string, llm.UsageRecord, error) {
	return f.response, llm.UsageRecord{Model: "test"}, f.err
}
func (f fakeClient) Close() error { return nil }

func TestParseResponseMapsLabelsAndClamps(t *testing.T) {
	result := ParseResponse("```json\n{\"classification\":\"confirmed\",\"confidence\":1.5,\"summary\":\"done\",\"extracted_fields\":{\"case\":\"42\"}}\n```")
	if result.Label != "confirmed" || result.EventType != "CONFIRMED" || result.Confidence != 1 || result.NeedsHumanReview {
		t.Fatalf("result = %#v", result)
	}
	if result.ExtractedFields["case"] != "42" {
		t.Fatalf("fields = %#v", result.ExtractedFields)
	}
}

func TestParseResponseMalformedAndLowConfidenceNeedReview(t *testing.T) {
	if result := ParseResponse("not json"); result.Label != "unclear" || !result.NeedsHumanReview {
		t.Fatalf("malformed = %#v", result)
	}
	result := ParseResponse(`{"classification":"ack","confidence":0.3,"summary":"low"}`)
	if result.EventType != "ACK" || !result.NeedsHumanReview {
		t.Fatalf("low confidence = %#v", result)
	}
}

func TestBuildUserPromptRedactsProfileValues(t *testing.T) {
	profile := &identity.Profile{FullName: "Jane Example", EmailAddresses: []string{"jane@example.com"}}
	prompt := BuildUserPrompt("Broker", "https://broker.example", "", "", "Reply", "Hello Jane Example jane@example.com", profile)
	if strings.Contains(prompt, "Jane Example") || strings.Contains(prompt, "jane@example.com") {
		t.Fatalf("PII leaked in prompt: %q", prompt)
	}
}

func TestReplyClassifierUsesInjectedClient(t *testing.T) {
	classifier := NewReplyClassifier(fakeClient{response: `{"classification":"ack","confidence":0.9,"summary":"received"}`}, nil)
	result, err := classifier.Classify(context.Background(), ClassifyOptions{BrokerName: "Broker", ReplyBody: "received"})
	if err != nil || result.Label != "ack" || result.Usage.Model != "test" {
		t.Fatalf("result=%#v err=%v", result, err)
	}
	_, err = NewReplyClassifier(fakeClient{err: errors.New("down")}, nil).Classify(context.Background(), ClassifyOptions{})
	if err == nil {
		t.Fatal("expected injected client error")
	}
}
