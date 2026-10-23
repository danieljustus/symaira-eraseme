// Package triage ports the deterministic parts of EraseMe's broker-reply
// triage adapter. LLM transport stays behind internal/llm.Client so tests can
// use a fake response without network access or credentials.
package triage

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/redaction"
)

const (
	ConfidenceThresholdLow  = 0.4
	ConfidenceThresholdHigh = 0.85
)

var ClassificationLabels = map[string]struct{}{
	"ack": {}, "confirmed": {}, "rejected": {}, "verification": {},
	"human_required": {}, "autoresponder": {}, "bounce": {}, "unclear": {},
}

var ClassificationToEvent = map[string]string{
	"ack": "ACK", "confirmed": "CONFIRMED", "rejected": "REJECTED_FINAL",
	"verification": "VERIFICATION_REQUESTED", "human_required": "HUMAN_ACTION_REQUIRED",
	"autoresponder": "AUTORESPONDER", "bounce": "BOUNCE", "unclear": "HUMAN_ACTION_REQUIRED",
}

const ClassifierSystemPrompt = `You are a precise email classifier for a data broker removal tool.

Your task is to classify incoming broker reply emails into one of these categories:

- ack: Broker acknowledges receipt. They are processing the request.
- confirmed: Broker confirms the data has been deleted or account closed.
- rejected: Broker explicitly rejects the request.
- verification: Broker asks for more information or identity verification.
- human_required: The reply requires manual human review.
- autoresponder: Automated out-of-office, delivery receipt, or receipt notice.
- bounce: Hard bounce — email address does not exist or mailbox is full.
- unclear: Cannot confidently classify into any of the above.

Respond with ONLY a JSON object on a single line:
{"classification": "<label>", "confidence": <0.0-1.0>, "summary": "<text>", "extracted_fields": {}}
If confidence < 0.4, use "unclear" as the fallback classification.`

// ClassificationResult is the persisted and displayed triage decision.
type ClassificationResult struct {
	Label            string          `json:"classification"`
	EventType        string          `json:"event_type"`
	Confidence       float64         `json:"confidence"`
	Summary          string          `json:"summary"`
	ExtractedFields  map[string]any  `json:"extracted_fields"`
	NeedsHumanReview bool            `json:"needs_human_review"`
	Usage            llm.UsageRecord `json:"-"`
}

// BuildUserPrompt mirrors classifier.build_user_prompt. Profile-aware
// redaction is applied before any caller hands the prompt to an LLM.
func BuildUserPrompt(brokerName, brokerWebsite, originalSubject, originalSnippet, replySubject, replyBody string, profile *identity.Profile) string {
	parts := []string{fmt.Sprintf("Broker: %s (%s)", brokerName, brokerWebsite)}
	if originalSubject != "" {
		parts = append(parts, "Original request subject: "+originalSubject)
	}
	if originalSnippet != "" {
		parts = append(parts, "Original request body (truncated):\n"+originalSnippet[:min(len(originalSnippet), 500)])
	}
	parts = append(parts, "\nReply subject: "+redaction.Redact(replySubject, profile))
	parts = append(parts, "Reply body:\n"+redaction.Redact(replyBody[:min(len(replyBody), 2000)], profile))
	return strings.Join(parts, "\n\n")
}

// ParseResponse parses the strict JSON response while tolerating markdown
// code fences, as the Python implementation does.
func ParseResponse(response string) ClassificationResult {
	result := ClassificationResult{Label: "unclear", EventType: ClassificationToEvent["unclear"], ExtractedFields: map[string]any{}, NeedsHumanReview: true}
	text := stripJSONCodeFence(response)
	var data map[string]any
	if err := json.Unmarshal([]byte(text), &data); err != nil {
		result.Summary = "Failed to parse classifier response"
		return result
	}
	label, _ := data["classification"].(string)
	label = strings.ToLower(strings.TrimSpace(label))
	if _, ok := ClassificationLabels[label]; !ok {
		label = "unclear"
	}
	confidence := number(data["confidence"])
	if confidence < 0 {
		confidence = 0
	}
	if confidence > 1 {
		confidence = 1
	}
	summary, _ := data["summary"].(string)
	if len(summary) > 200 {
		summary = summary[:200]
	}
	extracted, _ := data["extracted_fields"].(map[string]any)
	if extracted == nil {
		extracted = map[string]any{}
	}
	result.Label = label
	result.EventType = ClassificationToEvent[label]
	result.Confidence = confidence
	result.Summary = summary
	result.ExtractedFields = extracted
	result.NeedsHumanReview = confidence < ConfidenceThresholdLow || label == "unclear"
	return result
}

// ParseClassificationResponse is a descriptive alias used by service callers.
func ParseClassificationResponse(response string) ClassificationResult {
	return ParseResponse(response)
}

type ReplyClassifier struct {
	Client  llm.Client
	Profile *identity.Profile
}

func NewReplyClassifier(client llm.Client, profile *identity.Profile) *ReplyClassifier {
	return &ReplyClassifier{Client: client, Profile: profile}
}

func (c *ReplyClassifier) IsAvailable() bool {
	return c != nil && c.Client != nil && c.Client.IsAvailable()
}

func (c *ReplyClassifier) Classify(ctx context.Context, opts ClassifyOptions) (ClassificationResult, error) {
	if c == nil || c.Client == nil {
		return ClassificationResult{Label: "unclear", EventType: ClassificationToEvent["unclear"], NeedsHumanReview: true, ExtractedFields: map[string]any{}, Summary: "Classifier not initialized"}, nil
	}
	prompt := BuildUserPrompt(opts.BrokerName, opts.BrokerWebsite, opts.OriginalSubject, opts.OriginalSnippet, opts.ReplySubject, opts.ReplyBody, c.Profile)
	text, usage, err := c.Client.Classify(ctx, ClassifierSystemPrompt, prompt, llm.ClassifyOptions{CacheKey: opts.CacheKey})
	if err != nil {
		return ClassificationResult{Label: "unclear", EventType: ClassificationToEvent["unclear"], NeedsHumanReview: true, ExtractedFields: map[string]any{}, Summary: "API error: " + err.Error()}, err
	}
	result := ParseResponse(text)
	result.Usage = usage
	return result, nil
}

type ClassifyOptions struct {
	BrokerName, BrokerWebsite        string
	OriginalSubject, OriginalSnippet string
	ReplySubject, ReplyBody          string
	CacheKey                         string
}

func stripJSONCodeFence(text string) string {
	text = strings.TrimSpace(text)
	if !strings.HasPrefix(text, "```") {
		return text
	}
	text = strings.TrimPrefix(text, "```")
	text = strings.TrimSpace(text)
	if strings.HasPrefix(strings.ToLower(text), "json") {
		text = strings.TrimSpace(text[4:])
	}
	text = strings.TrimSpace(text)
	text = strings.TrimSuffix(text, "```")
	return strings.TrimSpace(text)
}

func number(value any) float64 {
	switch n := value.(type) {
	case float64:
		return n
	case float32:
		return float64(n)
	case int:
		return float64(n)
	case json.Number:
		v, _ := n.Float64()
		return v
	default:
		return 0
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
