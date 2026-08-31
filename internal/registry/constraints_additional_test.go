package registry

import (
	"strings"
	"testing"
)

func TestDecodeAndValidateRejectsFieldConstraints(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\ndata_sensitivity: 3\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"
	cases := []struct {
		name    string
		content string
		docID   string
	}{
		{"id pattern", strings.Replace(base, "id: test", "id: bad_id", 1), "bad_id"},
		{"id mismatch", strings.Replace(base, "id: test", "id: other", 1), "test"},
		{"empty name", strings.Replace(base, "name: Test", "name: \"\"", 1), "test"},
		{"missing website", strings.Replace(base, "website: https://example.test", "website: \"\"", 1), "test"},
		{"invalid category", strings.Replace(base, "category: other", "category: unknown", 1), "test"},
		{"empty jurisdictions", strings.Replace(base, "jurisdictions: [US]", "jurisdictions: []", 1), "test"},
		{"invalid jurisdiction", strings.Replace(base, "jurisdictions: [US]", "jurisdictions: [ZZ]", 1), "test"},
		{"empty laws", strings.Replace(base, "laws: [GDPR]", "laws: []", 1), "test"},
		{"invalid law", strings.Replace(base, "laws: [GDPR]", "laws: [INVALID]", 1), "test"},
		{"sensitivity too low", strings.Replace(base, "data_sensitivity: 3", "data_sensitivity: 0", 1), "test"},
		{"sensitivity too high", strings.Replace(base, "data_sensitivity: 3", "data_sensitivity: 6", 1), "test"},
		{"invalid priority", strings.Replace(base, "priority: low", "priority: urgent", 1), "test"},
		{"empty opt out", strings.Replace(base, "opt_out:\n  - type: email\n    endpoint: a@example.test\n", "opt_out: []\n", 1), "test"},
		{"invalid status", base + "status: pending\n", "test"},
		{"invalid date", base + "added_date: 31-08-2026\n", "test"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			doc := &doc{id: tc.docID, path: tc.docID + ".yaml", content: []byte(tc.content)}
			if _, err := decodeAndValidate(doc); err == nil {
				t.Fatalf("invalid document accepted: %s", tc.content)
			}
		})
	}
}
