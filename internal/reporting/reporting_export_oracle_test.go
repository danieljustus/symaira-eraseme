package reporting

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

const reportingExportFixturePath = "tests/fixtures/event-store/golden-reporting-exports.json"

type reportingExportFixture struct {
	ReportingSourceSHA256  string `json:"reporting_source_sha256"`
	TemplateSourceSHA256   string `json:"template_source_sha256"`
	TemplatingSourceSHA256 string `json:"templating_source_sha256"`
	JSON                   string `json:"json"`
	HTML                   string `json:"html"`
	HTMLSingleCampaign     string `json:"html_single_campaign"`
	HTMLEmptyCampaign      string `json:"html_empty_campaign"`
}

// TestReportingExportFixture captures the bytes returned by Go's real
// GenerateReport for a seeded nonempty report and caller-provided integers.
// Refresh with UPDATE_REPORTING_EXPORT_FIXTURE=1 go test ./internal/reporting -run '^TestReportingExportFixture$'.
func TestReportingExportFixture(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller")
	}
	root := filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
	store := fixtureStore(t)
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	data, err := GetReportData(context.Background(), store, ReportOpts{AllCampaigns: true, Now: now})
	if err != nil {
		t.Fatal(err)
	}
	// This key is also used by the report schema. Keeping its caller-provided
	// integer value verifies that export does not coerce arbitrary input types.
	data["success_rate"] = int64(42)
	data["caller_integral"] = int64(7)
	jsonText, err := GenerateReport(data, "json", now)
	if err != nil {
		t.Fatal(err)
	}
	htmlText, err := GenerateReport(data, "html", now)
	if err != nil {
		t.Fatal(err)
	}
	singleCampaign, err := GetReportData(context.Background(), store, ReportOpts{CampaignID: "new", Now: now})
	if err != nil {
		t.Fatal(err)
	}
	singleHTML, err := GenerateReport(singleCampaign, "html", now)
	if err != nil {
		t.Fatal(err)
	}
	for _, statement := range []string{
		`DELETE FROM request_events WHERE request_id=3`,
		`DELETE FROM request_state WHERE request_id=3`,
		`DELETE FROM removal_requests WHERE id=3`,
		`DELETE FROM campaigns WHERE id='old'`,
		`INSERT INTO campaigns(id,created_at,kind,notes) VALUES ('empty','2026-08-01T08:00:00+00:00','initial','empty')`,
	} {
		if _, err := store.DB().ExecContext(context.Background(), statement); err != nil {
			t.Fatalf("prepare empty-campaign report: %v", err)
		}
	}
	emptyCampaignData, err := GetReportData(context.Background(), store, ReportOpts{AllCampaigns: true, Now: now})
	if err != nil {
		t.Fatal(err)
	}
	emptyCampaignHTML, err := GenerateReport(emptyCampaignData, "html", now)
	if err != nil {
		t.Fatal(err)
	}
	readSHA256 := func(path string) string {
		t.Helper()
		raw, err := os.ReadFile(filepath.Join(root, path))
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(raw)
		return hex.EncodeToString(digest[:])
	}
	got := reportingExportFixture{
		ReportingSourceSHA256:  readSHA256("internal/reporting/reporting.go"),
		TemplateSourceSHA256:   readSHA256("internal/templating/templates/report.html.gotmpl"),
		TemplatingSourceSHA256: readSHA256("internal/templating/templating.go"),
		JSON:                   jsonText,
		HTML:                   htmlText,
		HTMLSingleCampaign:     singleHTML,
		HTMLEmptyCampaign:      emptyCampaignHTML,
	}
	path := filepath.Join(root, reportingExportFixturePath)
	if os.Getenv("UPDATE_REPORTING_EXPORT_FIXTURE") == "1" {
		encoded, err := json.MarshalIndent(got, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, append(encoded, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var want reportingExportFixture
	if err := json.Unmarshal(raw, &want); err != nil {
		t.Fatal(err)
	}
	if want != got {
		t.Fatalf("Go GenerateReport output differs from fixture; regenerate only after review\nwant hashes: %s %s\ngot hashes: %s %s", want.ReportingSourceSHA256, want.TemplateSourceSHA256, got.ReportingSourceSHA256, got.TemplateSourceSHA256)
	}
}
