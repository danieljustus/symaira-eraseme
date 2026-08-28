package reporting

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func fixtureStore(t *testing.T) *eventstore.Store {
	t.Helper()
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "symeraseme.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	ctx := context.Background()
	statements := []string{
		`INSERT INTO campaigns(id,created_at,kind,notes) VALUES ('new','2026-08-02T08:00:00+00:00','initial','new'),('old','2026-07-01T08:00:00+00:00','initial','old')`,
		`INSERT INTO removal_requests(id,broker_id,channel,campaign_id,created_at,jurisdiction,template_id,identity_snapshot_hash) VALUES
		 (1,'broker-a','email','new','2026-08-02T08:00:00+00:00','GDPR','gdpr','h'),
		 (2,'broker-b','web_form','new','2026-08-02T09:00:00+00:00','CCPA','ccpa','h'),
		 (3,'broker-a','email','old','2026-07-01T08:00:00+00:00','GDPR','gdpr','h')`,
		`INSERT INTO request_state(request_id,current_status,sent_at,resolved_at,deadline_at,next_action_at,reminders_sent,escalation_level,last_event_id,last_event_at)
		 VALUES (1,'CONFIRMED','2026-08-02T08:00:00+00:00','2026-08-04T08:00:00+00:00','2026-09-01T08:00:00+00:00',NULL,1,0,2,'2026-08-04T08:00:00+00:00'),
		 (2,'OVERDUE','2026-08-02T09:00:00+00:00',NULL,'2026-08-03T09:00:00+00:00','2026-08-05T09:00:00+00:00',2,2,4,'2026-08-05T09:00:00+00:00'),
		 (3,'REJECTED_FINAL','2026-07-01T08:00:00+00:00','2026-07-03T08:00:00+00:00','2026-07-31T08:00:00+00:00',NULL,0,0,6,'2026-07-03T08:00:00+00:00')`,
		`INSERT INTO request_events(id,request_id,event_type,occurred_at,payload_json,source) VALUES
		 (1,1,'SENT','2026-08-02T08:00:00+00:00','{}','system'),(2,1,'CONFIRMED','2026-08-04T08:00:00+00:00','{}','inbox'),
		 (3,2,'SENT','2026-08-02T09:00:00+00:00','{}','system'),(4,2,'DEADLINE_REACHED','2026-08-05T09:00:00+00:00','{}','scheduler'),
		 (5,3,'SENT','2026-07-01T08:00:00+00:00','{}','system'),(6,3,'REJECTED_FINAL','2026-07-03T08:00:00+00:00','{}','inbox')`,
	}
	for _, q := range statements {
		if _, err := store.DB().ExecContext(ctx, q); err != nil {
			t.Fatalf("seed: %v", err)
		}
	}
	return store
}

func TestReportDashboardCalendarParityShape(t *testing.T) {
	store := fixtureStore(t)
	ctx := context.Background()
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	report, err := GetReportData(ctx, store, ReportOpts{AllCampaigns: true, Now: now})
	if err != nil {
		t.Fatal(err)
	}
	if report["total_campaigns"] != 2 || report["total_requests"] != 3 {
		t.Fatalf("totals: %#v", report)
	}
	metrics := report["success_metrics"].(map[string]any)
	if metrics["overall_confirmation_rate"] != 33.3 || metrics["median_response_time_days"] != 2.0 {
		t.Fatalf("metrics: %#v", metrics)
	}
	comparison := report["historical_comparison"].(map[string]any)
	if comparison["requests_change"] != 1 {
		t.Fatalf("comparison: %#v", comparison)
	}
	dashboard, err := GetDashboardData(ctx, store, "", now)
	if err != nil {
		t.Fatal(err)
	}
	if dashboard["total_requests"] != 3 || dashboard["confirmed"] != 1 || dashboard["overdue"] != 1 {
		t.Fatalf("dashboard: %#v", dashboard)
	}
	calendar, err := GetCalendar(ctx, store, "", 4, now)
	if err != nil {
		t.Fatal(err)
	}
	totals := calendar["totals"].(map[string]any)
	if totals["entries"] != 1 || totals["overdue"] != 1 {
		t.Fatalf("calendar: %#v", calendar)
	}
	status, err := GetCampaignStatus(ctx, store, "", now)
	if err != nil {
		t.Fatal(err)
	}
	st := status["totals"].(map[string]any)
	if st["requests"] != 3 || st["resolved"] != 2 {
		t.Fatalf("status: %#v", status)
	}
}

func TestReportExports(t *testing.T) {
	store := fixtureStore(t)
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	data, err := GetReportData(context.Background(), store, ReportOpts{AllCampaigns: true, Now: now})
	if err != nil {
		t.Fatal(err)
	}
	js, err := ExportJSON(data)
	if err != nil {
		t.Fatal(err)
	}
	var decoded map[string]any
	if err := json.Unmarshal([]byte(js), &decoded); err != nil {
		t.Fatal(err)
	}
	if strings.HasSuffix(js, "\n") {
		t.Fatal("JSON must not end with newline")
	}
	csvText, err := ExportCSV(data)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(csvText, "campaign_id,request_id,broker_id") || !strings.Contains(csvText, "\r\n") {
		t.Fatalf("csv: %q", csvText)
	}
	html, err := GenerateReport(data, "html", now)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(html, "Campaign Report") || !strings.Contains(html, "broker-a") {
		t.Fatalf("html missing data")
	}
	dashboard, err := GetDashboardData(context.Background(), store, "", now)
	if err != nil {
		t.Fatal(err)
	}
	page, err := GenerateDashboard(dashboard, 30, now)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(page, `http-equiv="refresh" content="30"`) {
		t.Fatalf("dashboard refresh missing")
	}
	if _, err := GenerateReport(data, "pdf", now); err == nil || err.Error() != "unsupported format: pdf; choose html, json, or csv" {
		t.Fatalf("error = %v", err)
	}
}

func TestGoldenReportingConformance(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller")
	}
	root := filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
	raw, err := os.ReadFile(filepath.Join(root, "tests", "fixtures", "event-store", "golden-reporting.json"))
	if err != nil {
		t.Fatal(err)
	}
	var want map[string]any
	if err := json.Unmarshal(raw, &want); err != nil {
		t.Fatal(err)
	}
	store := fixtureStore(t)
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	report, _ := GetReportData(context.Background(), store, ReportOpts{AllCampaigns: true, Now: now})
	dashboard, _ := GetDashboardData(context.Background(), store, "", now)
	status, _ := GetCampaignStatus(context.Background(), store, "", now)
	calendar, _ := GetCalendar(context.Background(), store, "", 4, now)
	got := map[string]any{"report": report, "dashboard": dashboard, "campaign_status": status, "calendar": calendar}
	for _, key := range []string{"report", "dashboard", "campaign_status", "calendar"} {
		gotJSON, _ := json.Marshal(got[key])
		wantJSON, _ := json.Marshal(want[key])
		if string(gotJSON) != string(wantJSON) {
			t.Fatalf("%s mismatch\ngot:  %s\nwant: %s", key, gotJSON, wantJSON)
		}
	}
}
