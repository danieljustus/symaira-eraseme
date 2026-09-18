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
	if err := store.Close(); err != nil {
		fail(err)
	}

	// Only the cases that carry no wall-clock value are recorded. `list` and the
	// `show` detail block embed the task's `created_at`, which Go fills from
	// `time.Now()` with no injection point, so pinning them would bake a moving
	// value into the fixture. They are covered by shape assertions instead.
	cases := []fixtureCase{
		{Name: "manual_tasks_show_reports_a_missing_task", Request: callRequest(3, "manual_tasks_show", `{"task_id":999}`)},
		{Name: "manual_tasks_complete_marks_the_task_completed", Request: callRequest(4, "manual_tasks_complete", fmt.Sprintf(`{"task_id":%d,"notes":"done by hand"}`, task.ID))},
		{Name: "manual_tasks_cleanup_reports_a_missing_directory", Request: callRequest(5, "manual_tasks_cleanup", `{"dry_run":true}`)},
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
