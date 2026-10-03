// Current-source byte responses from the real MCP server and production handler.
// Each case uses disposable configuration and a private store; no browser,
// scheduler installation, mailbox or paid provider is invoked.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

type testCase struct {
	Name      string         `json:"name"`
	Tool      string         `json:"tool"`
	Arguments map[string]any `json:"arguments"`
	Response  string         `json:"response"`
	Error     bool           `json:"error"`
	BadStore  bool           `json:"bad_store,omitempty"`
}

func main() {
	sourceRoot, err := os.Getwd()
	must(err)
	sources := map[string]string{}
	for _, name := range []string{"go.mod", "go.sum", "internal/eventstore/store.go", "internal/eventstore/encrypt.go", "internal/config/storage.go", "internal/mcp/contract_handler.go", "internal/mcp/server.go", "internal/reporting/reporting.go", "rust-tests/parity/oracle/mcp-tool-gaps/main.go"} {
		raw, err := os.ReadFile(filepath.Join(sourceRoot, name))
		must(err)
		hash := sha256.Sum256(raw)
		sources[name] = hex.EncodeToString(hash[:])
	}
	systemRoot := os.Getenv("SystemRoot")
	os.Clearenv()
	if systemRoot != "" {
		must(os.Setenv("SystemRoot", systemRoot))
	}
	root, err := os.MkdirTemp("", "mcp-tool-gaps-")
	must(err)
	defer os.RemoveAll(root)
	cases := []testCase{
		{Name: "list-brokers-entire-registry", Tool: "list_brokers", Arguments: map[string]any{"include_inactive": true, "include_disabled": true}},
		{Name: "list-brokers-populated", Tool: "list_brokers", Arguments: map[string]any{"jurisdiction": "DE"}},
		{Name: "list-brokers-empty", Tool: "list_brokers", Arguments: map[string]any{"jurisdiction": "zz-not-a-region"}},
		{Name: "dashboard-empty-html", Tool: "generate_dashboard", Arguments: map[string]any{"auto_refresh": 0}},
		{Name: "report-empty-html", Tool: "generate_report", Arguments: map[string]any{"format": "html", "all_campaigns": true}},
		{Name: "report-invalid-format", Tool: "generate_report", Arguments: map[string]any{"format": "not-a-format", "all_campaigns": true}, Error: true},
		{Name: "web-form-preview", Tool: "run_web_form", Arguments: map[string]any{"broker_id": "redfin-us", "dry_run": true}},
		{Name: "web-form-missing-broker", Tool: "run_web_form", Arguments: map[string]any{"broker_id": "absent-synthetic-broker", "dry_run": true}},
		{Name: "auto-confirm-missing-reply", Tool: "auto_confirm", Arguments: map[string]any{"request_id": 999, "dry_run": true}},
	}
	cases = append(cases, testCase{Name: "auto-confirm-invalid-request", Tool: "auto_confirm", Arguments: map[string]any{"request_id": "bad", "dry_run": true}, Error: true})
	for _, tool := range []string{"schedule_install", "schedule_status", "schedule_uninstall"} {
		cases = append(cases, testCase{Name: tool + "-unsupported-platform", Tool: tool, Arguments: map[string]any{"platform": "not-a-platform"}, Error: true})
	}
	cases = append(cases, testCase{Name: "list-brokers-invalid-boolean", Tool: "list_brokers", Arguments: map[string]any{"include_disabled": "yes"}, Error: true})
	for _, tool := range []string{"get_calendar", "get_dashboard_data", "generate_dashboard", "generate_report", "plan_show", "list_requests", "get_events", "manual_tasks_list", "manual_tasks_show", "manual_tasks_complete", "manual_tasks_cleanup"} {
		arguments := map[string]any{}
		if tool == "get_events" {
			arguments["request_id"] = 1
		}
		if tool == "manual_tasks_show" || tool == "manual_tasks_complete" {
			arguments["task_id"] = 1
		}
		cases = append(cases, testCase{Name: tool + "-corrupt-database", Tool: tool, Arguments: arguments, BadStore: true, Error: tool != "manual_tasks_cleanup"})
	}
	instant, err := time.Parse(time.RFC3339, "2026-08-05T12:00:00Z")
	must(err)
	for i := range cases {
		directory := filepath.Join(root, cases[i].Name)
		must(os.MkdirAll(directory, 0700))
		must(os.Chdir(directory))
		must(os.Setenv("HOME", directory))
		must(os.Setenv("USERPROFILE", directory))
		must(os.Setenv("SYMERASEME_DATA_DIR", filepath.Join(directory, "data")))
		for _, key := range []string{"XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "TMP", "TEMP", "TMPDIR"} {
			must(os.Setenv(key, directory))
		}
		store, err := eventstore.Open(filepath.Join(directory, "data", "symeraseme.db"))
		must(err)
		must(store.Close())
		if cases[i].BadStore {
			database := filepath.Join(directory, "data", "symeraseme.db")
			must(os.WriteFile(database, []byte("synthetic corrupt SQLite database"), 0600))
		}
		frame, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": map[string]any{"name": cases[i].Tool, "arguments": cases[i].Arguments}})
		must(err)
		var output bytes.Buffer
		handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{Now: func() time.Time { return instant }})
		must(mcp.NewServer(handler).ServeStdio(context.Background(), bytes.NewReader(append(frame, '\n')), &output))
		cases[i].Response = output.String()
		var observed map[string]any
		must(json.Unmarshal(output.Bytes(), &observed))
		_, failed := observed["error"]
		if failed != cases[i].Error {
			panic(fmt.Sprintf("%s did not reach intended result/error class", cases[i].Name))
		}
	}
	must(os.Chdir(sourceRoot))
	must(json.NewEncoder(os.Stdout).Encode(map[string]any{"schema": "symeraseme.go-oracle.mcp-tool-gaps.v1", "go_version": runtime.Version(), "now": "2026-08-05T12:00:00Z", "sources_sha256": sources, "cases": cases}))
}
func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
