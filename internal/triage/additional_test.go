package triage

import (
	"context"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type unavailableReplyClient struct{}

func (unavailableReplyClient) IsAvailable() bool { return false }
func (unavailableReplyClient) Classify(context.Context, string, string, llm.ClassifyOptions) (string, llm.UsageRecord, error) {
	return "", llm.UsageRecord{}, nil
}
func (unavailableReplyClient) Close() error { return nil }

func TestParseResponseNormalizesUnknownValuesAndBoundsSummary(t *testing.T) {
	result := ParseResponse(`{"classification":"not-a-label","confidence":-2,"summary":"` + strings.Repeat("x", 250) + `","extracted_fields":"wrong"}`)
	if result.Label != "unclear" || result.EventType != "HUMAN_ACTION_REQUIRED" || result.Confidence != 0 || !result.NeedsHumanReview {
		t.Fatalf("normalized result = %#v", result)
	}
	if len(result.Summary) != 200 || len(result.ExtractedFields) != 0 {
		t.Fatalf("bounded fields = %#v", result)
	}
	result = ParseResponse(`{"classification":"ack","confidence":2,"extracted_fields":{}}`)
	if result.Confidence != 1 || result.NeedsHumanReview {
		t.Fatalf("high confidence result = %#v", result)
	}
}

func TestBuildUserPromptIncludesOptionalSectionsAndTruncates(t *testing.T) {
	original := strings.Repeat("o", 600)
	body := strings.Repeat("b", 2100)
	prompt := BuildUserPrompt("Broker", "https://broker.example", "Subject", original, "Reply", body, nil)
	if !strings.Contains(prompt, "Original request subject: Subject") || !strings.Contains(prompt, "Reply subject: Reply") {
		t.Fatalf("optional prompt sections missing: %q", prompt)
	}
	if strings.Contains(prompt, strings.Repeat("o", 501)) || strings.Contains(prompt, strings.Repeat("b", 2001)) {
		t.Fatal("prompt did not truncate bounded sections")
	}
}

func TestReplyClassifierAvailabilityCoversNilAndUnavailableClients(t *testing.T) {
	var nilClassifier *ReplyClassifier
	if nilClassifier.IsAvailable() {
		t.Fatal("nil classifier reported available")
	}
	classifier := NewReplyClassifier(unavailableReplyClient{}, nil)
	if classifier.IsAvailable() {
		t.Fatal("unavailable client reported available")
	}
	result, err := classifier.Classify(context.Background(), ClassifyOptions{})
	if err != nil || result.Label != "unclear" {
		t.Fatalf("nil-client classification = %#v, err=%v", result, err)
	}
}

func TestRejectionHelpersNormalizeAndSelectFallbacks(t *testing.T) {
	for _, tc := range []struct {
		message string
		want    string
	}{
		{"The old address is wrong", "address_mismatch"},
		{"Please verify your identity", "identity_challenged"},
		{"California CCPA request", "ccpa_identity_challenged"},
		{"No matching reason", ""},
	} {
		if got := SelectFallbackTemplate(tc.message); got != tc.want {
			t.Errorf("fallback for %q = %q, want %q", tc.message, got, tc.want)
		}
	}
	prompt := BuildRebuttalClassifierPrompt("", strings.Repeat("x", 3200), strings.Repeat("o", 600), nil)
	if !strings.Contains(prompt, "Broker: Unknown") || len(prompt) > 3600 {
		t.Fatalf("bounded rebuttal prompt = %d chars", len(prompt))
	}
	parsed := ParseRejectionClassification(`{"classification":"unexpected","confidence":4,"summary":"` + strings.Repeat("s", 250) + `","key_points":["one",4]}`)
	if parsed.Classification != "other" || parsed.Confidence != 1 || len(parsed.Summary) != 200 || len(parsed.KeyPoints) != 1 {
		t.Fatalf("normalized rejection = %#v", parsed)
	}
	if parsed := ParseRejectionClassification("invalid"); parsed.Classification != "other" || parsed.Jurisdiction != "unknown" {
		t.Fatalf("invalid rejection = %#v", parsed)
	}
}
