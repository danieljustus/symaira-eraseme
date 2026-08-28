package templating

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

// repoRoot walks up from the package dir to the module root.
func repoRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("go.mod not found")
		}
		dir = parent
	}
}

// fixtureProfile mirrors the IdentityProfile the Python generator used.
func fixtureProfile() *identity.Profile {
	dob := "1990-06-15"
	return &identity.Profile{
		FullName:     "Max Mustermann",
		NameVariants: []string{"Max M.", "M. Mustermann"},
		DateOfBirth:  &dob,
		Addresses: []identity.Address{
			{Street: "Teststrasse 1", City: "Berlin", PostalCode: "10115", Country: "DE"},
			{Street: "Alte Adresse 2", City: "Muenchen", PostalCode: "80331", Country: "DE"},
		},
		EmailAddresses: []string{"max@example.com", "max.m@example.org"},
		PhoneNumbers:   []string{"+49 30 123456"},
		Jurisdictions:  []string{"DE", "EU"},
	}
}

// fixtureData mirrors the "data" dict the Python generator rendered.
func fixtureData() map[string]any {
	req := func(id int, broker, juris, status, sent, resolved string, reminders int) map[string]any {
		return map[string]any{
			"id": id, "broker_id": broker, "jurisdiction": juris,
			"current_status": status, "sent_at": sent, "resolved_at": resolved,
			"reminders_sent": reminders,
		}
	}
	return map[string]any{
		"total_requests":    3,
		"planned":           1,
		"sent":              1,
		"awaiting_ack":      0,
		"awaiting_response": 0,
		"confirmed":         1,
		"rejected":          0,
		"overdue":           0,
		"campaigns": []any{
			map[string]any{
				"campaign_id": "q3-2026", "id": "q3-2026", "kind": "initial",
				"created_at": "2026-07-01T09:00:00+00:00", "total": 3,
				"confirmation_rate": 33, "planned": 1, "sent": 1,
				"awaiting_ack": 0, "awaiting_response": 0, "confirmed": 1,
				"rejected": 0, "overdue": 0, "total_reminders_sent": 4,
				"avg_response_time_days": 12,
				"requests": []any{
					req(1, "acxiom-eu", "DE", "CONFIRMED", "2026-07-02T08:00:00+00:00", "2026-07-20T10:30:00+00:00", 2),
					req(2, "oracle-data", "US", "PLANNED", "", "", 0),
				},
			},
		},
		"broker_status": []any{
			map[string]any{"broker_id": "acxiom-eu", "total": 2, "confirmed": 1, "pending": 0, "overdue": 0, "rejected": 0},
		},
		"recent_events": []any{
			map[string]any{"occurred_at": "2026-07-20T10:30:00+00:00", "event_type": "CONFIRMED", "request_id": 1, "broker_id": "acxiom-eu", "source": "inbox"},
			map[string]any{"occurred_at": "2026-07-02T08:00:00+00:00", "event_type": "SENT", "request_id": 1, "broker_id": "acxiom-eu", "source": "system"},
		},
		"success_metrics": map[string]any{
			"overall_confirmation_rate": 33, "overall_rejection_rate": 0, "overdue_rate": 0,
			"avg_response_time_days": 12, "median_response_time_days": 12,
		},
		"historical_comparison": map[string]any{
			"requests_change": "+10", "confirmation_rate_change": 5, "rejection_rate_change": -2,
		},
		"broker_leaderboard": []any{
			map[string]any{"broker_id": "acxiom-eu", "total": 3, "confirmed": 2, "rejected": 0, "overdue": 0, "success_rate": 66, "avg_response_time_days": 12},
		},
		"jurisdiction_stats": []any{
			map[string]any{"jurisdiction": "DE", "total": 2, "confirmed": 2, "rejected": 0, "overdue": 0, "confirmation_rate": 100},
		},
		"timeline": []any{
			map[string]any{"date": "2026-07-20", "total_events": 3, "events": []string{"ACK", "CONFIRMED", "NOTE_ADDED"}},
		},
	}
}

// now matches the Python generator's datetime(2026, 8, 27, 14, 30, UTC).
var now = time.Date(2026, 8, 27, 14, 30, 0, 0, time.UTC)

func loadGoldenTemplates(t *testing.T) map[string]string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(repoRoot(t), "tests", "fixtures", "event-store", "golden-templates.json"))
	if err != nil {
		t.Fatalf("read golden templates: %v", err)
	}
	var golden map[string]string
	if err := json.Unmarshal(b, &golden); err != nil {
		t.Fatalf("parse golden templates: %v", err)
	}
	if len(golden) != 11 {
		t.Fatalf("golden fixture: got %d templates, want 11", len(golden))
	}
	return golden
}

// TestGoldenTemplateConformance renders EVERY template with the identical
// fixture inputs the Python generator used and requires byte-identical
// output.  This is the legal-correctness gate: a conversion drift in any
// letter would silently ship wrong opt-out text.
func TestGoldenTemplateConformance(t *testing.T) {
	golden := loadGoldenTemplates(t)
	profile := fixtureProfile()
	data := fixtureData()
	extra := map[string]any{
		"original_request_date": "2026-07-20",
		"request_id":            "REQ-12345",
		"broker_reply_snippet":  "We could not verify your identity.",
		"auto_refresh_seconds":  0,
		"data":                  data,
		"now":                   now,
	}

	letterExtra := map[string]any{
		"original_request_date": extra["original_request_date"],
		"request_id":            extra["request_id"],
		"broker_reply_snippet":  extra["broker_reply_snippet"],
	}

	for name, want := range golden {
		t.Run(name, func(t *testing.T) {
			opts := RenderOpts{
				Profile:       profile,
				BrokerName:    "Acme Data Corp",
				BrokerWebsite: "https://acme.example.com",
			}
			if name == "templates/dashboard.html.j2" {
				opts.ExtraVars = map[string]any{
					"auto_refresh_seconds": extra["auto_refresh_seconds"],
					"data":                 data,
					"now":                  now,
				}
			} else if name == "templates/report.html.j2" {
				opts.ExtraVars = map[string]any{"data": data, "now": now}
			} else {
				opts.ExtraVars = letterExtra
			}
			got, err := Render(name, opts)
			if err != nil {
				t.Fatalf("Render %s: %v", name, err)
			}
			if got != want {
				// Find first divergence for a useful diff.
				minLen := len(got)
				if len(want) < minLen {
					minLen = len(want)
				}
				i := 0
				for i < minLen && got[i] == want[i] {
					i++
				}
				t.Errorf("%s mismatch (len got=%d want=%d, first diff at %d)\n got: %q\nwant: %q",
					name, len(got), len(want), i, got[max(0, i-40):min(len(got), i+80)], want[max(0, i-40):min(len(want), i+80)])
			}
		})
	}
}

// TestListTemplateNames covers the Python list_templates surface.
func TestListTemplateNames(t *testing.T) {
	names := ListTemplateNames()
	if len(names) != 11 {
		t.Fatalf("got %d names, want 11: %v", len(names), names)
	}
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
