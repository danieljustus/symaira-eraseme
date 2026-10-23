package manualtasks

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

// Result is the JSON-safe service response consumed by the future MCP server.
type Result struct {
	Success bool           `json:"success"`
	Data    map[string]any `json:"-"`
	Error   string         `json:"error,omitempty"`
}

// MarshalJSON mirrors CliResult.to_json(): service data is flattened into the
// top-level payload consumed by both the CLI JSON mode and MCP tool responses.
func (result Result) MarshalJSON() ([]byte, error) {
	payload := map[string]any{"success": result.Success}
	if result.Error != "" {
		payload["error"] = result.Error
	}
	for key, value := range result.Data {
		if result.Error != "" && key == "message" {
			continue
		}
		payload[key] = value
	}
	return json.Marshal(payload)
}

// HandleList implements manual_tasks_list.
func HandleList(ctx context.Context, store *eventstore.Store, opts ListOpts) (Result, error) {
	tasks, err := List(ctx, store, opts)
	if err != nil {
		return Result{}, err
	}
	data := map[string]any{"tasks": tasks}
	if len(tasks) == 0 {
		data["message"] = "No manual tasks found."
		return Result{Success: true, Data: data}, nil
	}
	message := fmt.Sprintf("Manual tasks (%d):", len(tasks))
	for _, task := range tasks {
		broker := task.BrokerName
		if broker == "" {
			broker = task.BrokerID
		}
		message += fmt.Sprintf("\n  #%d [%s] %s (%s) @ %s", task.ID, task.Status, broker, task.Reason, task.CreatedAt)
	}
	data["message"] = message
	return Result{Success: true, Data: data}, nil
}

// HandleShow implements manual_tasks_show.
func HandleShow(ctx context.Context, store *eventstore.Store, taskID int64) (Result, error) {
	task, err := Get(ctx, store, taskID)
	if err != nil {
		return Result{}, err
	}
	if task == nil {
		return Result{Success: false, Error: fmt.Sprintf("Manual task #%d not found. Run 'symeraseme manual-tasks list' to see available tasks.", taskID)}, nil
	}
	data := taskMap(*task)
	message := fmt.Sprintf("Manual task #%d:\n  Broker:     %s (%s)\n  URL:        %s\n  Reason:     %s\n  Status:     %s\n  Created:    %s",
		task.ID, task.BrokerName, task.BrokerID, task.FormURL, task.Reason, task.Status, task.CreatedAt)
	if task.CompletedAt != nil && *task.CompletedAt != "" {
		message += "\n  Completed:  " + *task.CompletedAt
	}
	if task.ScreenshotPath != "" {
		message += "\n  Screenshot: " + task.ScreenshotPath
	}
	if task.HTMLSnapshotPath != "" {
		message += "\n  HTML:       " + task.HTMLSnapshotPath
	}
	message += "\n\nInstructions:\n" + task.Instructions
	if task.Notes != "" {
		message += "\n\nNotes: " + task.Notes
	}
	data["message"] = message
	return Result{Success: true, Data: data}, nil
}

// HandleComplete implements manual_tasks_complete.
func HandleComplete(ctx context.Context, store *eventstore.Store, taskID int64, notes string) (Result, error) {
	task, err := Complete(ctx, store, taskID, notes, true)
	if err != nil {
		return Result{}, err
	}
	if task == nil {
		return Result{Success: false, Error: fmt.Sprintf("Manual task #%d not found. Run 'symeraseme manual-tasks list' to see available tasks.", taskID)}, nil
	}
	return Result{Success: true, Data: map[string]any{
		"task_id": taskID, "message": fmt.Sprintf("Manual task #%d marked as completed.", taskID),
	}}, nil
}

// HandleCleanup implements manual_tasks_cleanup.
func HandleCleanup(dryRun bool) (Result, error) {
	dir, err := TasksDir()
	if err != nil {
		return Result{}, err
	}
	if _, statErr := os.Stat(dir); errors.Is(statErr, os.ErrNotExist) {
		return Result{Success: true, Data: map[string]any{"message": "No manual tasks directory found — nothing to clean up."}}, nil
	} else if statErr != nil {
		return Result{}, statErr
	}
	result, err := Cleanup(dir, dryRun)
	if err != nil {
		return Result{}, err
	}
	data := map[string]any{"removed": result.Removed, "skipped": result.Skipped, "dry_run": result.DryRun}
	if dryRun {
		data["message"] = fmt.Sprintf("Would remove %d artifact(s) from %s. Use --yes to confirm.", result.Skipped, dir)
	} else {
		data["message"] = fmt.Sprintf("Removed %d artifact(s) from %s.", result.Removed, dir)
	}
	return Result{Success: true, Data: data}, nil
}

func taskMap(task ManualTask) map[string]any {
	return map[string]any{
		"id": task.ID, "request_id": task.RequestID, "broker_id": task.BrokerID,
		"broker_name": task.BrokerName, "form_url": task.FormURL, "reason": task.Reason,
		"instructions": task.Instructions, "screenshot_path": task.ScreenshotPath,
		"html_snapshot_path": task.HTMLSnapshotPath, "form_fields_json": task.FormFieldsJSON,
		"status": task.Status, "created_at": task.CreatedAt, "completed_at": task.CompletedAt,
		"notes": task.Notes,
	}
}
