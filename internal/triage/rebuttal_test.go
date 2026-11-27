package triage

import (
	"context"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func TestRejectionParserAndFallbackAreJurisdictionAware(t *testing.T) {
	parsed := ParseRejectionClassification("```json\n{\"classification\":\"ccpa_identity_challenged\",\"confidence\":0.91,\"jurisdiction\":\"CCPA\",\"summary\":\"verify\",\"key_points\":[\"ID\"]}\n```")
	if parsed.Classification != "ccpa_identity_challenged" || parsed.Jurisdiction != "CCPA" || len(parsed.KeyPoints) != 1 {
		t.Fatalf("parsed = %#v", parsed)
	}
	if got := SelectFallbackTemplate("Our CCPA team needs California identity verification"); got != "identity_challenged" {
		t.Fatalf("fallback = %q", got)
	}
}

func TestGenerateRebuttalUsesConvertedTemplateWithoutLLM(t *testing.T) {
	result, err := GenerateRebuttal(context.Background(), RebuttalOptions{
		BrokerName: "Example Broker", BrokerMessage: "Your request was rejected because of an old address",
		OriginalRequestDate: "2026-08-28", Profile: &identity.Profile{FullName: "Test Person", EmailAddresses: []string{"test@example.com"}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.Jurisdiction != "GDPR" || result.TemplateName != "gdpr-rebuttal-address.md.j2" || result.LLMUsed {
		t.Fatalf("result = %#v", result)
	}
	if !strings.Contains(result.RebuttalBody, "old address") || !result.NeedsHumanReview {
		t.Fatalf("body/result = %#v", result)
	}
}
