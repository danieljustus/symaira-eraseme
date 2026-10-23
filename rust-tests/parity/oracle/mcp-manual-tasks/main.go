// Command mcp-manual-tasks is the byte oracle for the MCP manual-task tools.
//
// It wires the real `mcp.ContractHandler()` and seeds an isolated data
// directory, so every recorded response is the production answer for that
// store rather than a stub's.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "42614bc27527711baec9b3e9d2805ce1e1dee185"
	sourcePath     = "internal/mcp/contract_handler.go:167-320, internal/manualtasks/service.go:37-135"
)

type fixtureCase struct {
	Name     string  `json:"name"`
	Request  string  `json:"request"`
	Response *string `json:"response"`
}

func callRequest(id int, name string, arguments string) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}`, id, name, arguments)
}

func main() {
	workspace, err := os.MkdirTemp("", "mcp-manual-tasks")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	// An isolated data directory inside the workspace: the store-backed tools
	// resolve their database from here instead of the developer's real store.
	dataDir := filepath.Join(workspace, "data")
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		fail(err)
	}
	if err := os.Setenv("SYMERASEME_DATA_DIR", dataDir); err != nil {
		fail(err)
	}

	storage, err := config.ResolveStorage()
	if err != nil {
		fail(err)
	}
	if err := os.MkdirAll(storage.DBDir, 0o700); err != nil {
		fail(err)
	}
	store, err := eventstore.Open(storage.DBPath)
	if err != nil {
		fail(err)
	}
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker-a", "web_form", "mcp-003", "CCPA", "", "")
	if err != nil {
		fail(err)
	}
	task, err := manualtasks.Create(context.Background(), store, manualtasks.CreateOpts{
		RequestID:  &requestID,
		BrokerID:   "broker-a",
		BrokerName: "Broker A",
		FormURL:    "https://broker-a.example/optout",
		Reason:     "captcha_failed",
	})
	if err != nil {
		fail(err)
	}
	if _, err := store.DB().Exec(`UPDATE manual_tasks SET created_at = ? WHERE id = ?`, time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC), task.ID); err != nil {
		fail(err)
	}
	if err := store.Close(); err != nil {
		fail(err)
	}

	tasksDir, err := manualtasks.TasksDir()
	if err != nil {
		fail(err)
	}
	if err := os.MkdirAll(tasksDir, 0o700); err != nil {
		fail(err)
	}
	for name, contents := range map[string]string{"evidence.png": "artifact", "keep.txt": "keep"} {
		if err := os.WriteFile(filepath.Join(tasksDir, name), []byte(contents), 0o600); err != nil {
			fail(err)
		}
	}

	cases := []fixtureCase{
		{Name: "manual_tasks_list_returns_the_populated_queue", Request: callRequest(1, "manual_tasks_list", `{}`)},
		{Name: "manual_tasks_show_returns_the_task_details", Request: callRequest(2, "manual_tasks_show", fmt.Sprintf(`{"task_id":%d}`, task.ID))},
		{Name: "manual_tasks_show_reports_a_missing_task", Request: callRequest(3, "manual_tasks_show", `{"task_id":999}`)},
		{Name: "manual_tasks_complete_marks_the_task_completed", Request: callRequest(4, "manual_tasks_complete", fmt.Sprintf(`{"task_id":%d,"notes":"done by hand"}`, task.ID))},
		{Name: "manual_tasks_complete_reports_a_missing_task", Request: callRequest(5, "manual_tasks_complete", `{"task_id":999}`)},
		{Name: "manual_tasks_cleanup_counts_artifacts_without_removing_them", Request: callRequest(6, "manual_tasks_cleanup", `{"dry_run":true}`)},
		{Name: "manual_tasks_cleanup_removes_artifacts", Request: callRequest(7, "manual_tasks_cleanup", `{}`)},
		{Name: "manual_tasks_cleanup_keeps_unrelated_files", Request: callRequest(8, "manual_tasks_cleanup", `{}`)},
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	for index := range cases {
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		err := server.ServeStdio(context.Background(), bytes.NewReader(append([]byte(cases[index].Request), '\n')), &output)
		if err != nil {
			fail(err)
		}
		if body := output.String(); body != "" {
			body = strings.ReplaceAll(body, workspace, "ORACLE_ROOT")
			cases[index].Response = &body
		}
	}
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Cases: cases}); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
