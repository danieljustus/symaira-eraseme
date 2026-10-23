package mcp

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestContractHandlerCoversReadOnlyToolDispatch(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	input := filepath.Join(dataDir, "input.txt")
	if err := os.WriteFile(input, []byte("Contact jane@example.test"), 0o600); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	handler := ContractHandler()
	cases := []struct {
		name string
		args map[string]any
	}{
		{"redact_file", map[string]any{"path": input}},
		{"plan_show", map[string]any{"campaign_id": "missing"}},
		{"execute dry run", map[string]any{"campaign_id": "missing", "dry_run": true}},
		{"poll inbox invalid adapter", map[string]any{"host": "imap.example.test", "port": 993, "username": "user@example.test", "since_days": 1, "ssl": true}},
		{"classify reply unavailable provider", map[string]any{"request_id": 999, "provider": "unknown"}},
		{"generate rebuttal unavailable provider", map[string]any{"request_id": 999, "provider": "unknown"}},
		{"generate dashboard", map[string]any{}},
		{"generate report", map[string]any{"all_campaigns": true}},
		{"manual tasks list", map[string]any{}},
		{"manual tasks show", map[string]any{"task_id": 999}},
		{"manual tasks complete", map[string]any{"task_id": 999}},
		{"manual tasks cleanup", map[string]any{"dry_run": true}},
		{"generate scheduler", map[string]any{"platform": "cron", "dry_run": true, "poll_hours": "8,20"}},
		{"schedule install dry run", map[string]any{"platform": "cron", "dry_run": true}},
		{"schedule status", map[string]any{"platform": "cron"}},
		{"validate registry", map[string]any{}},
		{"run web form dry run", map[string]any{"broker_id": "missing", "dry_run": true}},
		{"auto confirm no reply", map[string]any{"request_id": 999, "dry_run": true}},
		{"dashboard data", map[string]any{}},
		{"list requests", map[string]any{"page": 1, "page_size": 10}},
		{"get events", map[string]any{"request_id": 999}},
		{"list brokers", map[string]any{"jurisdiction": "DE"}},
		{"get calendar", map[string]any{"weeks": 1}},
		{"grant dry run", map[string]any{"command": "execute", "dry_run": true}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, _ = handler(ctx, toolNameForTest(tc.name), tc.args)
		})
	}
}

func toolNameForTest(name string) string {
	mapping := map[string]string{
		"redact_file": "redact_file", "plan_show": "plan_show", "execute dry run": "execute",
		"poll inbox invalid adapter": "poll_inbox", "classify reply unavailable provider": "classify_reply",
		"generate rebuttal unavailable provider": "generate_rebuttal", "generate dashboard": "generate_dashboard",
		"generate report": "generate_report", "manual tasks list": "manual_tasks_list", "manual tasks show": "manual_tasks_show",
		"manual tasks complete": "manual_tasks_complete", "manual tasks cleanup": "manual_tasks_cleanup", "generate scheduler": "generate_scheduler",
		"schedule install dry run": "schedule_install", "schedule status": "schedule_status", "validate registry": "validate",
		"run web form dry run": "run_web_form", "auto confirm no reply": "auto_confirm", "dashboard data": "get_dashboard_data",
		"list requests": "list_requests", "get events": "get_events", "list brokers": "list_brokers", "get calendar": "get_calendar",
		"grant dry run": "grant",
	}
	return mapping[name]
}
