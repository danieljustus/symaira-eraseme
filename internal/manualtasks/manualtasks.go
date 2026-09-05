// Package manualtasks ports the manual fallback queue from
// src/symeraseme/core/manual_fallback.py and its repository layer.
package manualtasks

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

var fallbackReasons = map[string]struct{}{
	"unknown_captcha": {}, "captcha_failed": {}, "timeout": {},
	"login_required": {}, "multi_step_exceeded": {}, "dynamic_form": {},
	"unknown_field": {}, "assertion_failed": {}, "generic_error": {},
}

// FallbackReasons returns the supported fallback reasons in stable order.
var FallbackReasons = []string{
	"unknown_captcha", "captcha_failed", "timeout", "login_required",
	"multi_step_exceeded", "dynamic_form", "unknown_field", "assertion_failed",
	"generic_error",
}

// FormState is the browser state captured when automation stops.
type FormState struct {
	URL            string            `json:"url"`
	ScreenshotPath string            `json:"screenshot_path,omitempty"`
	HTMLSnapshot   string            `json:"html_snapshot"`
	FormFields     map[string]string `json:"form_fields"`
	FieldSelectors []string          `json:"field_selectors"`
	ErrorMessage   string            `json:"error_message"`
	Reason         string            `json:"reason"`
	StepIndex      int               `json:"step_index"`
	TotalSteps     int               `json:"total_steps"`
	BrokerName     string            `json:"broker_name"`
	BrokerID       string            `json:"broker_id"`
}

// ManualTask is the durable manual-fallback queue record.
type ManualTask struct {
	ID               int64   `json:"id"`
	RequestID        *int64  `json:"request_id"`
	BrokerID         string  `json:"broker_id"`
	BrokerName       string  `json:"broker_name"`
	FormURL          string  `json:"form_url"`
	Reason           string  `json:"reason"`
	Instructions     string  `json:"instructions"`
	ScreenshotPath   string  `json:"screenshot_path"`
	HTMLSnapshotPath string  `json:"html_snapshot_path"`
	FormFieldsJSON   string  `json:"form_fields_json"`
	Status           string  `json:"status"`
	CreatedAt        string  `json:"created_at"`
	CompletedAt      *string `json:"completed_at"`
	Notes            string  `json:"notes"`
}

// CreateOpts contains the captured browser state needed to create a task.
type CreateOpts struct {
	RequestID         *int64
	BrokerID          string
	BrokerName        string
	FormURL           string
	Reason            string
	ScreenshotPath    string
	HTMLSnapshot      string
	FormFields        map[string]string
	StepIndex         int
	TotalSteps        int
	ErrorMessage      string
	ExtraInstructions string
	Profile           *identity.Profile
}

// ListOpts controls optional queue filters.
type ListOpts struct {
	Status    *string
	RequestID *int64
}

// CleanupResult reports artifact cleanup without deleting task rows.
type CleanupResult struct {
	Removed int  `json:"removed"`
	Skipped int  `json:"skipped"`
	DryRun  bool `json:"dry_run"`
}

var (
	emailPattern = regexp.MustCompile(`\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z|a-z]{2,}\b`)
	phonePattern = regexp.MustCompile(`\b\d{3}[-.\s]?\d{3}[-.\s]?\d{4}\b`)
)

// RedactIdentityValues removes profile values from an HTML snapshot. When no
// profile is supplied it applies the same coarse email/phone fallback as Python.
func RedactIdentityValues(html string, profile *identity.Profile) string {
	redacted := html
	if profile != nil {
		for _, value := range profile.EmailAddresses {
			redacted = replaceNonEmpty(redacted, value, "[REDACTED-EMAIL]")
		}
		for _, value := range profile.PhoneNumbers {
			redacted = replaceNonEmpty(redacted, value, "[REDACTED-PHONE]")
		}
		redacted = replaceNonEmpty(redacted, profile.FullName, "[REDACTED-NAME]")
		for _, value := range profile.NameVariants {
			redacted = replaceNonEmpty(redacted, value, "[REDACTED-NAME]")
		}
		if profile.DateOfBirth != nil {
			redacted = replaceNonEmpty(redacted, *profile.DateOfBirth, "[REDACTED-DOB]")
		}
		for _, address := range profile.Addresses {
			redacted = replaceNonEmpty(redacted, address.Street, "[REDACTED-STREET]")
			redacted = replaceNonEmpty(redacted, address.City, "[REDACTED-CITY]")
			redacted = replaceNonEmpty(redacted, address.PostalCode, "[REDACTED-POSTAL]")
			if address.State != nil {
				redacted = replaceNonEmpty(redacted, *address.State, "[REDACTED-STATE]")
			}
		}
		return redacted
	}
	redacted = emailPattern.ReplaceAllString(redacted, "[REDACTED-EMAIL]")
	return phonePattern.ReplaceAllString(redacted, "[REDACTED-PHONE]")
}

func replaceNonEmpty(value, sensitive, marker string) string {
	if sensitive == "" {
		return value
	}
	return strings.ReplaceAll(value, sensitive, marker)
}

func redactFields(fields map[string]string, profile *identity.Profile) map[string]string {
	redacted := make(map[string]string, len(fields))
	for key, value := range fields {
		redacted[key] = RedactIdentityValues(value, profile)
	}
	return redacted
}

// TasksDir returns the Python-compatible manual task artifact directory.
func TasksDir() (string, error) {
	if value := os.Getenv("SYMERASEME_DATA_DIR"); value != "" {
		return filepath.Join(expandHome(value), "manual_tasks"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "symeraseme", "manual_tasks"), nil
}

func ensureTasksDir(path string) error {
	if err := os.MkdirAll(path, 0o700); err != nil {
		return err
	}
	return os.Chmod(path, 0o700)
}

func expandHome(path string) string {
	if path == "~" {
		if home, err := os.UserHomeDir(); err == nil {
			return home
		}
	}
	if strings.HasPrefix(path, "~/") {
		if home, err := os.UserHomeDir(); err == nil {
			return filepath.Join(home, path[2:])
		}
	}
	return path
}

func instructionsForReason(reason, brokerName string) string {
	instructions := map[string]string{
		"unknown_captcha":     fmt.Sprintf("The web form for %s has an unknown CAPTCHA type that could not be solved automatically. Please visit the URL below and complete the CAPTCHA manually, then submit the form.", brokerName),
		"captcha_failed":      fmt.Sprintf("The CAPTCHA solver failed for %s's opt-out form. Please visit the URL below and complete the CAPTCHA manually.", brokerName),
		"timeout":             fmt.Sprintf("The web form for %s timed out during submission. This may indicate a slow server or a multi-step process. Please visit the URL below and complete the opt-out process manually.", brokerName),
		"login_required":      fmt.Sprintf("The web form for %s requires login or authentication. Automatic form filling cannot proceed. Please log in and complete the opt-out process manually.", brokerName),
		"multi_step_exceeded": fmt.Sprintf("The web form for %s requires more steps than the configured limit. Please visit the URL below and follow the opt-out process to completion.", brokerName),
		"dynamic_form":        fmt.Sprintf("The web form for %s uses dynamic JavaScript fields that could not be filled automatically. Please visit the URL and complete the form manually.", brokerName),
		"unknown_field":       fmt.Sprintf("The web form for %s contains unknown fields that could not be mapped from the identity profile. Please visit the URL and complete the form manually.", brokerName),
		"assertion_failed":    fmt.Sprintf("The web form for %s was submitted but the expected confirmation message was not displayed. Please visit the URL and verify whether the opt-out was successful.", brokerName),
		"generic_error":       fmt.Sprintf("An unexpected error occurred while processing the web form for %s. Please visit the URL below and complete the opt-out process manually.", brokerName),
	}
	if value, ok := instructions[reason]; ok {
		return value
	}
	return fmt.Sprintf("Please complete the opt-out process for %s manually by visiting the URL below.", brokerName)
}

// Create persists a pending task, stores a redacted snapshot, and records the
// HUMAN_ACTION_REQUIRED event when a request id is present.
func Create(ctx context.Context, store *eventstore.Store, opts CreateOpts) (ManualTask, error) {
	if store == nil {
		return ManualTask{}, errors.New("manualtasks: store is required")
	}
	reason := opts.Reason
	if _, ok := fallbackReasons[reason]; !ok {
		reason = "generic_error"
	}
	formURL := RedactIdentityValues(opts.FormURL, opts.Profile)
	screenshotPath := RedactIdentityValues(opts.ScreenshotPath, opts.Profile)
	instructions := instructionsForReason(reason, opts.BrokerName)
	if opts.ExtraInstructions != "" {
		instructions += "\n\n" + RedactIdentityValues(opts.ExtraInstructions, opts.Profile)
	}
	formFieldsJSON, err := json.Marshal(redactFields(nonNilFields(opts.FormFields), opts.Profile))
	if err != nil {
		return ManualTask{}, err
	}
	tx, err := store.DB().BeginTx(ctx, nil)
	if err != nil {
		return ManualTask{}, err
	}
	rollback := true
	defer func() {
		if rollback {
			_ = tx.Rollback()
		}
	}()
	requestArg := nullableRequestID(opts.RequestID)
	existing, err := scanTask(tx.QueryRowContext(ctx, `SELECT id, request_id, broker_id, broker_name,
		form_url, reason, instructions, screenshot_path, html_snapshot_path,
		form_fields_json, status, created_at, completed_at, notes
		FROM manual_tasks
		WHERE status = 'pending' AND broker_id = ? AND form_url = ? AND reason = ?
		  AND ((request_id IS NULL AND ? IS NULL) OR request_id = ?)
		ORDER BY id LIMIT 1`, opts.BrokerID, formURL, reason, requestArg, requestArg))
	if err == nil {
		return existing, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return ManualTask{}, err
	}
	htmlPath := ""
	tasksDir, err := TasksDir()
	if err != nil {
		return ManualTask{}, err
	}
	if opts.HTMLSnapshot != "" {
		if err := ensureTasksDir(tasksDir); err != nil {
			return ManualTask{}, err
		}
		htmlPath = filepath.Join(tasksDir, fmt.Sprintf("snapshot_%d.html", time.Now().UnixNano()))
		if err := os.WriteFile(htmlPath, []byte(RedactIdentityValues(opts.HTMLSnapshot, opts.Profile)), 0o600); err != nil {
			// Python logs snapshot failures and continues with an empty path.
			htmlPath = ""
		} else if err := os.Chmod(htmlPath, 0o600); err != nil {
			_ = os.Remove(htmlPath)
			htmlPath = ""
		}
	}
	removeHTML := htmlPath != ""
	defer func() {
		if rollback && removeHTML {
			_ = os.Remove(htmlPath)
		}
	}()
	result, err := tx.ExecContext(ctx, `INSERT INTO manual_tasks
		(request_id, broker_id, broker_name, form_url, reason, instructions,
		 screenshot_path, html_snapshot_path, form_fields_json, status)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')`,
		requestArg, opts.BrokerID, opts.BrokerName, formURL,
		reason, instructions, screenshotPath, htmlPath, string(formFieldsJSON))
	if err != nil {
		return ManualTask{}, err
	}
	taskID, err := result.LastInsertId()
	if err != nil {
		return ManualTask{}, err
	}
	createdAt := time.Now().UTC().Format("2006-01-02T15:04:05")
	if opts.RequestID != nil && *opts.RequestID != 0 {
		_, _, err = eventstore.AppendAndProjectTx(ctx, tx, *opts.RequestID, eventstore.EvtHumanActionRequired, map[string]any{
			"manual_task_id": taskID, "reason": reason, "form_url": formURL,
			"instructions": instructions, "screenshot_path": screenshotPath,
			"broker_name": opts.BrokerName, "step_index": opts.StepIndex,
			"total_steps": opts.TotalSteps, "error_message": RedactIdentityValues(opts.ErrorMessage, opts.Profile),
		}, eventstore.SrcSystem, time.Now().UTC())
		if err != nil {
			return ManualTask{}, err
		}
	}
	if err := tx.Commit(); err != nil {
		return ManualTask{}, err
	}
	rollback = false
	return ManualTask{ID: taskID, RequestID: opts.RequestID, BrokerID: opts.BrokerID,
		BrokerName: opts.BrokerName, FormURL: formURL, Reason: reason,
		Instructions: instructions, ScreenshotPath: screenshotPath,
		HTMLSnapshotPath: htmlPath, FormFieldsJSON: string(formFieldsJSON),
		Status: "pending", CreatedAt: createdAt, Notes: ""}, nil
}

func nonNilFields(fields map[string]string) map[string]string {
	if fields == nil {
		return map[string]string{}
	}
	return fields
}

func nullableRequestID(value *int64) any {
	if value == nil {
		return nil
	}
	return *value
}

// Get retrieves one task, returning (nil, nil) when absent.
func Get(ctx context.Context, store *eventstore.Store, taskID int64) (*ManualTask, error) {
	row := store.DB().QueryRowContext(ctx, `SELECT id, request_id, broker_id, broker_name,
		form_url, reason, instructions, screenshot_path, html_snapshot_path,
		form_fields_json, status, created_at, completed_at, notes
		FROM manual_tasks WHERE id = ?`, taskID)
	task, err := scanTask(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &task, nil
}

// List returns tasks newest first, matching the Python repository ordering.
func List(ctx context.Context, store *eventstore.Store, opts ListOpts) ([]ManualTask, error) {
	query := `SELECT id, request_id, broker_id, broker_name, form_url, reason,
		instructions, screenshot_path, html_snapshot_path, form_fields_json,
		status, created_at, completed_at, notes FROM manual_tasks WHERE 1=1`
	args := []any{}
	if opts.Status != nil && *opts.Status != "" {
		query += " AND status = ?"
		args = append(args, *opts.Status)
	}
	if opts.RequestID != nil {
		query += " AND request_id = ?"
		args = append(args, *opts.RequestID)
	}
	query += " ORDER BY created_at DESC"
	rows, err := store.DB().QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]ManualTask, 0)
	for rows.Next() {
		task, err := scanTaskRows(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, task)
	}
	return out, rows.Err()
}

type rowScanner interface{ Scan(...any) error }

func scanTask(row rowScanner) (ManualTask, error)     { return scanTaskValues(row) }
func scanTaskRows(row rowScanner) (ManualTask, error) { return scanTaskValues(row) }

func scanTaskValues(row rowScanner) (ManualTask, error) {
	var task ManualTask
	var requestID sql.NullInt64
	var completedAt sql.NullString
	if err := row.Scan(&task.ID, &requestID, &task.BrokerID, &task.BrokerName,
		&task.FormURL, &task.Reason, &task.Instructions, &task.ScreenshotPath,
		&task.HTMLSnapshotPath, &task.FormFieldsJSON, &task.Status, &task.CreatedAt,
		&completedAt, &task.Notes); err != nil {
		return ManualTask{}, err
	}
	if requestID.Valid {
		task.RequestID = &requestID.Int64
	}
	if completedAt.Valid {
		task.CompletedAt = &completedAt.String
	}
	return task, nil
}

// Complete marks a task completed or cancelled and records NOTE_ADDED.
func Complete(ctx context.Context, store *eventstore.Store, taskID int64, notes string, completed bool) (*ManualTask, error) {
	if store == nil {
		return nil, errors.New("manualtasks: store is required")
	}
	tx, err := store.DB().BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()
	taskValue, err := scanTask(tx.QueryRowContext(ctx, `SELECT id, request_id, broker_id, broker_name,
		form_url, reason, instructions, screenshot_path, html_snapshot_path,
		form_fields_json, status, created_at, completed_at, notes
		FROM manual_tasks WHERE id = ?`, taskID))
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	task := &taskValue
	status := "cancelled"
	if completed {
		status = "completed"
	}
	completedAt := time.Now().UTC().Format("2006-01-02T15:04:05")
	if _, err := tx.ExecContext(ctx,
		"UPDATE manual_tasks SET status = ?, completed_at = ?, notes = ? WHERE id = ?",
		status, completedAt, notes, taskID); err != nil {
		return nil, err
	}
	if task.RequestID != nil && *task.RequestID != 0 {
		_, _, err = eventstore.AppendAndProjectTx(ctx, tx, *task.RequestID, eventstore.EvtNoteAdded, map[string]any{
			"note":           fmt.Sprintf("Manual task #%d %s: %s", taskID, status, notes),
			"manual_task_id": taskID, "form_url": task.FormURL,
		}, eventstore.SrcUser, time.Now().UTC())
		if err != nil {
			return nil, err
		}
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	task.Status, task.CompletedAt, task.Notes = status, &completedAt, notes
	return task, nil
}

// Cleanup removes .png, .html, and .json artifacts from the task directory.
// This intentionally includes task JSON rows, matching the Python handler.
func Cleanup(tasksDir string, dryRun bool) (CleanupResult, error) {
	var result CleanupResult
	result.DryRun = dryRun
	entries, err := os.ReadDir(tasksDir)
	if errors.Is(err, os.ErrNotExist) {
		return result, nil
	}
	if err != nil {
		return result, err
	}
	for _, entry := range entries {
		if entry.IsDir() || !isArtifact(entry.Name()) {
			continue
		}
		if dryRun {
			result.Skipped++
			continue
		}
		if err := os.Remove(filepath.Join(tasksDir, entry.Name())); err != nil {
			return result, err
		}
		result.Removed++
	}
	return result, nil
}

func isArtifact(name string) bool {
	return strings.HasSuffix(name, ".png") || strings.HasSuffix(name, ".html") || strings.HasSuffix(name, ".json")
}

// SaveScreenshot stores a screenshot in the manual-task artifact directory
// with restrictive permissions and returns its path. Empty screenshots are
// ignored so evidence capture remains best effort.
func SaveScreenshot(data []byte) (string, error) {
	if len(data) == 0 {
		return "", nil
	}
	dir, err := TasksDir()
	if err != nil {
		return "", err
	}
	if err := ensureTasksDir(dir); err != nil {
		return "", err
	}
	path := filepath.Join(dir, fmt.Sprintf("screenshot_%d.png", time.Now().UnixNano()))
	if err := os.WriteFile(path, data, 0o600); err != nil {
		return "", err
	}
	if err := os.Chmod(path, 0o600); err != nil {
		return "", err
	}
	return path, nil
}

// InstructionsForReason is exported for parity tests and UI callers.
func InstructionsForReason(reason, brokerName, _ string) string {
	return instructionsForReason(reason, brokerName)
}
