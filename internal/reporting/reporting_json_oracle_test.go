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

const reportingJSONFixturePath = "tests/fixtures/event-store/golden-reporting-bytes.json"

type reportingJSONFixture struct {
	SourceSHA256   string `json:"source_sha256"`
	Report         string `json:"report"`
	Dashboard      string `json:"dashboard"`
	CampaignStatus string `json:"campaign_status"`
	Calendar       string `json:"calendar"`
}

// TestReportingJSONBytesFixture records Go encoding/json's exact bytes for the
// four public reporting maps. Refresh deliberately with
// UPDATE_REPORTING_JSON_FIXTURE=1 go test ./internal/reporting -run '^TestReportingJSONBytesFixture$'.
func TestReportingJSONBytesFixture(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller")
	}
	root := filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
	store := fixtureStore(t)
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	report, err := GetReportData(context.Background(), store, ReportOpts{AllCampaigns: true, Now: now})
	if err != nil {
		t.Fatal(err)
	}
	dashboard, err := GetDashboardData(context.Background(), store, "", now)
	if err != nil {
		t.Fatal(err)
	}
	status, err := GetCampaignStatus(context.Background(), store, "", now)
	if err != nil {
		t.Fatal(err)
	}
	calendar, err := GetCalendar(context.Background(), store, "", 4, now)
	if err != nil {
		t.Fatal(err)
	}
	encode := func(value any) string {
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return string(encoded)
	}
	source, err := os.ReadFile(filepath.Join(root, "internal", "reporting", "reporting.go"))
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(source)
	got := reportingJSONFixture{
		SourceSHA256:   hex.EncodeToString(digest[:]),
		Report:         encode(report),
		Dashboard:      encode(dashboard),
		CampaignStatus: encode(status),
		Calendar:       encode(calendar),
	}
	path := filepath.Join(root, reportingJSONFixturePath)
	if os.Getenv("UPDATE_REPORTING_JSON_FIXTURE") == "1" {
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
	var want reportingJSONFixture
	if err := json.Unmarshal(raw, &want); err != nil {
		t.Fatal(err)
	}
	if want != got {
		t.Fatalf("Go reporting JSON bytes differ from %s", reportingJSONFixturePath)
	}
}
