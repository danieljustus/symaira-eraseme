package triage

import (
	"context"
	"encoding/json"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/redaction"
	"github.com/danieljustus/symaira-eraseme/internal/templating"
)

type RejectionTemplate struct {
	Template     string
	Label        string
	Description  string
	Jurisdiction string
}

var RejectionTemplates = map[string]RejectionTemplate{
	"address_mismatch":         {"gdpr-rebuttal-address.md.j2", "Address Discrepancy Rebuttal (GDPR)", "Broker rejected request citing old address on file", "GDPR"},
	"identity_challenged":      {"gdpr-rebuttal-identity.md.j2", "Identity Verification Rebuttal (GDPR)", "Broker challenged identity verification", "GDPR"},
	"ccpa_identity_challenged": {"ccpa-rebuttal-deletion.md.j2", "Identity Verification Rebuttal (CCPA)", "Broker requested additional identity info under CCPA", "CCPA"},
}

var fallbackKeywords = []struct {
	Keywords []string
	Key      string
}{
	{[]string{"address", "old address", "previous address", "current address"}, "address_mismatch"},
	{[]string{"identity", "verify", "verification", "id", "passport", "driver"}, "identity_challenged"},
	{[]string{"ccpa", "california", "section 1798"}, "ccpa_identity_challenged"},
}

const RebuttalSystemPrompt = `You are a precise rejection classifier for a data broker removal tool.

Classify the broker response as address_mismatch, identity_challenged,
ccpa_identity_challenged, or other. Respond with only JSON:
{"classification":"<label>","confidence":0.0,"summary":"<text>","key_points":[],"jurisdiction":"<GDPR|CCPA|unknown>"}`

type RejectionClassification struct {
	Classification string   `json:"classification"`
	Confidence     float64  `json:"confidence"`
	Summary        string   `json:"summary"`
	KeyPoints      []string `json:"key_points"`
	Jurisdiction   string   `json:"jurisdiction"`
}

type RebuttalResult struct {
	TemplateName            string
	Label                   string
	Description             string
	Jurisdiction            string
	RejectionClassification string
	Confidence              float64
	RebuttalBody            string
	NeedsHumanReview        bool
	LLMUsed                 bool
	Usage                   llm.UsageRecord
}

func SelectFallbackTemplate(message string) string {
	lower := strings.ToLower(message)
	for _, item := range fallbackKeywords {
		for _, keyword := range item.Keywords {
			if strings.Contains(lower, strings.ToLower(keyword)) {
				return item.Key
			}
		}
	}
	return ""
}

func BuildRebuttalClassifierPrompt(brokerName, message, originalTemplate string, profile *identity.Profile) string {
	parts := []string{"Broker: " + defaultString(brokerName, "Unknown")}
	if originalTemplate != "" {
		parts = append(parts, "Original request (truncated):\n"+originalTemplate[:min(len(originalTemplate), 500)])
	}
	message = message[:min(len(message), 3000)]
	parts = append(parts, "\nBroker response:\n"+redaction.Redact(message, profile))
	return strings.Join(parts, "\n\n")
}

func ParseRejectionClassification(response string) RejectionClassification {
	result := RejectionClassification{Classification: "other", Jurisdiction: "unknown"}
	var data map[string]any
	if json.Unmarshal([]byte(stripJSONCodeFence(response)), &data) != nil {
		result.Summary = "Failed to parse classifier response"
		return result
	}
	classification, _ := data["classification"].(string)
	classification = strings.ToLower(strings.TrimSpace(classification))
	if _, ok := RejectionTemplates[classification]; !ok && classification != "other" {
		classification = "other"
	}
	result.Classification = classification
	result.Confidence = number(data["confidence"])
	if result.Confidence < 0 {
		result.Confidence = 0
	}
	if result.Confidence > 1 {
		result.Confidence = 1
	}
	result.Summary, _ = data["summary"].(string)
	if len(result.Summary) > 200 {
		result.Summary = result.Summary[:200]
	}
	result.Jurisdiction, _ = data["jurisdiction"].(string)
	if result.Jurisdiction == "" {
		result.Jurisdiction = "unknown"
	}
	if points, ok := data["key_points"].([]any); ok {
		for _, point := range points {
			if value, ok := point.(string); ok {
				result.KeyPoints = append(result.KeyPoints, value)
			}
		}
	}
	return result
}

type RebuttalOptions struct {
	BrokerName, BrokerWebsite                                   string
	BrokerMessage, OriginalRequestTemplate, OriginalRequestDate string
	Profile                                                     *identity.Profile
	Client                                                      llm.Client
}

func GenerateRebuttal(ctx context.Context, opts RebuttalOptions) (RebuttalResult, error) {
	classification := RejectionClassification{}
	llmUsed := false
	var usage llm.UsageRecord
	if opts.Client != nil && opts.Client.IsAvailable() {
		prompt := BuildRebuttalClassifierPrompt(opts.BrokerName, opts.BrokerMessage, opts.OriginalRequestTemplate, opts.Profile)
		text, used, err := opts.Client.Classify(ctx, RebuttalSystemPrompt, prompt, llm.ClassifyOptions{CacheKey: "rebuttal:" + opts.BrokerName})
		if err == nil {
			classification = ParseRejectionClassification(text)
			usage = used
			llmUsed = true
		}
	}

	key := classification.Classification
	if !llmUsed || key == "other" {
		key = SelectFallbackTemplate(opts.BrokerMessage)
	}
	if key == "" {
		key = "identity_challenged"
	}
	info := RejectionTemplates[key]
	name := info.Template
	available := templating.ListTemplateNames()
	if !containsString(available, name) {
		prefix := strings.ToLower(info.Jurisdiction)
		for _, candidate := range available {
			if strings.HasPrefix(candidate, prefix) {
				name = candidate
				break
			}
		}
	}
	if !containsString(available, name) {
		name = "gdpr-art17.en.md.j2"
		info = RejectionTemplate{Template: name, Label: "GDPR Erasure Request (Fallback)", Description: "Fallback template when rebuttal template not found", Jurisdiction: "GDPR"}
	}
	body, err := templating.Render(name, templating.RenderOpts{
		Profile: opts.Profile, BrokerName: opts.BrokerName, BrokerWebsite: opts.BrokerWebsite,
		ExtraVars: map[string]any{"original_request_date": opts.OriginalRequestDate},
	})
	if err != nil {
		return RebuttalResult{}, err
	}
	needsReview := !llmUsed || classification.Classification == "other" || classification.Confidence < 0.5
	return RebuttalResult{
		TemplateName: name, Label: info.Label, Description: info.Description, Jurisdiction: info.Jurisdiction,
		RejectionClassification: ternaryString(classification.Classification, "fallback", llmUsed),
		Confidence:              classification.Confidence, RebuttalBody: body, NeedsHumanReview: needsReview,
		LLMUsed: llmUsed, Usage: usage,
	}, nil
}

func containsString(values []string, needle string) bool {
	for _, value := range values {
		if value == needle {
			return true
		}
	}
	return false
}
func defaultString(value, fallback string) string {
	if value != "" {
		return value
	}
	return fallback
}
func ternaryString(value, fallback string, use bool) string {
	if use {
		return value
	}
	return fallback
}
