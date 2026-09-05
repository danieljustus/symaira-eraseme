// Port of src/symeraseme/core/execution.py — single-request execution.
package campaign

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
)

// Errors mirror core/exceptions.py for the executed domain.
var (
	// ErrRequestNotFound is raised when no removal request exists for an id.
	ErrRequestNotFound = errors.New("campaign: removal request not found")
	// ErrWebFormRunnerRequired mirrors the Python ValueError.
	ErrWebFormRunnerRequired = errors.New("campaign: web_form_runner is required for web_form requests")
	// ErrEmailSenderRequired mirrors the Python ValueError.
	ErrEmailSenderRequired = errors.New("campaign: email_sender is required for email-based requests")
	// ErrIdentityProfileNotFound is the missing-profile flavour of
	// ProfileError (email execution requires a profile).
	ErrIdentityProfileNotFound = errors.New("campaign: identity profile not found — run 'symeraseme init-profile' first")
)

// ExecutionError wraps a send failure, mirroring ExecutionError in
// core/exceptions.py (carries request_id).
type ExecutionError struct {
	RequestID int64
	Msg       string
}

func (e *ExecutionError) Error() string {
	return fmt.Sprintf("campaign: request %d: %s", e.RequestID, e.Msg)
}

// ExecuteOpts bundles the injectable adapters for ExecuteRequest.
type ExecuteOpts struct {
	Account     string
	ConfigPath  string
	DryRun      bool
	WebForm     WebFormRunner
	Email       EmailSender
	Render      TemplateRenderer
	ProfilePath string // "" = platform default
}

// ExecuteRequest mirrors core/execution.execute_request(): dispatches on
// the request's channel and writes the SENT/SEND_FAILED outcome to the
// event store.  In dry-run mode no adapter is required and no events are
// appended — which is what keeps it genuinely side-effect free.
func ExecuteRequest(ctx context.Context, store *eventstore.Store, requestID int64, opts ExecuteOpts) (map[string]any, error) {
	req, err := store.GetRemovalRequest(ctx, requestID)
	if err != nil {
		return nil, err
	}
	if req == nil {
		return nil, fmt.Errorf("%w: %d", ErrRequestNotFound, requestID)
	}

	brokerName, _ := req["broker_id"].(string)
	channelType, _ := req["channel"].(string)
	if channelType == "" {
		channelType = "email"
	}

	if channelType == "web_form" {
		return executeWebformRequest(ctx, store, brokerName, opts.WebForm, opts.DryRun, requestID, opts.ProfilePath)
	}

	// Email path: build payload from the last event (mirrors Python).
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		return nil, err
	}
	var payload map[string]any
	if len(events) > 0 {
		payload = events[len(events)-1].Payload
	}
	templateID, _ := req["template_id"].(string)

	return executeEmailRequest(ctx, store, requestID, brokerName, payload, templateID, opts)
}

// executeWebformRequest mirrors the Python _execute_webform_request.
func executeWebformRequest(
	ctx context.Context,
	store *eventstore.Store,
	brokerName string,
	runner WebFormRunner,
	dryRun bool,
	requestID int64,
	profilePath string,
) (map[string]any, error) {
	if runner == nil {
		if dryRun {
			return map[string]any{
				"success":    true,
				"request_id": requestID,
				"dry_run":    true,
				"url":        "",
				"body":       fmt.Sprintf("[dry-run web form for %s]", brokerName),
			}, nil
		}
		fallback, fallbackErr := manualWebFormTaskResult(ctx, store, brokerName, requestID)
		if fallbackErr != nil {
			return nil, fallbackErr
		}
		runner = func(context.Context, string, bool) map[string]any { return fallback }
	}

	profile, identityHash, err := loadWebFormProfile(profilePath)
	if err != nil {
		return nil, err
	}

	rawResult := runner(ctx, brokerName, dryRun)
	result := sanitizeWebFormResult(rawResult, profile)
	if success, _ := result["success"].(bool); success {
		_, _, err = storeAppend(ctx, store, requestID, eventstore.EvtSent, map[string]any{
			"broker_name":            brokerName,
			"form_url":               result["url"],
			"expected_response_days": 30,
			"identity_snapshot_hash": identityHash,
			"formflow_code":          result["code"],
			"evidence":               result["evidence"],
		})
	} else {
		payload := map[string]any{
			"error":         result["error"],
			"broker_name":   brokerName,
			"formflow_code": result["code"],
			"reason":        stringOr(result["reason"], reasonForCodeString(result["code"])),
			"evidence":      result["evidence"],
		}
		if result["final_url"] != nil {
			payload["form_url"] = result["final_url"]
		} else {
			payload["form_url"] = result["url"]
		}

		if taskID, ok := result["task_id"]; ok && taskID != nil {
			payload["task_id"] = taskID
		} else {
			// The Python web-form service creates the task before execution
			// records SEND_FAILED. Keep that ordering so HUMAN_ACTION_REQUIRED
			// projects first, then SEND_FAILED becomes the final status.
			// Raw screenshots cannot be safely redacted at this boundary.
			screenshotPath := ""
			htmlSnapshot := stringValue(rawResult["html_snapshot"])
			if htmlSnapshot == "" {
				htmlSnapshot = stringValue(rawResult["page_text"])
			}
			formURL := stringValue(result["url"])
			if formURL == "" {
				formURL = stringValue(result["final_url"])
			}
			task, taskErr := manualtasks.Create(ctx, store, manualtasks.CreateOpts{
				RequestID:      &requestID,
				BrokerID:       brokerName,
				BrokerName:     brokerName,
				FormURL:        formURL,
				Reason:         stringOr(result["reason"], reasonForCodeString(result["code"])),
				ScreenshotPath: screenshotPath,
				HTMLSnapshot:   htmlSnapshot,
				FormFields:     stringMap(rawResult["form_fields"]),
				StepIndex:      intValue(rawResult["step_index"]),
				TotalSteps:     intValue(rawResult["total_steps"]),
				ErrorMessage:   stringValue(rawResult["error"]),
				Profile:        profile,
			})
			if taskErr != nil {
				return nil, taskErr
			}
			payload["task_id"] = task.ID
			result["task_id"] = task.ID
		}
		_, _, err = storeAppend(ctx, store, requestID, eventstore.EvtSendFailed, payload)
	}

	if err != nil {
		return nil, err
	}
	out := map[string]any{"success": result["success"], "request_id": requestID}
	for k, v := range result {
		out[k] = v
	}
	return out, nil
}

// sanitizeWebFormResult preserves deterministic evidence metadata without
// returning or persisting raw page text, screenshots, form values, or paths.
func sanitizeWebFormResult(raw map[string]any, profile *identity.Profile) map[string]any {
	if raw == nil {
		return map[string]any{"success": false, "code": string(CodeInteractionFailed), "error": "web form executor returned no result"}
	}
	result := make(map[string]any, len(raw))
	for key, value := range raw {
		switch key {
		case "evidence":
			if evidence := sanitizeEvidenceMap(value, profile); len(evidence) > 0 {
				result[key] = evidence
			}
		case "page_text", "html_snapshot", "screenshot_path", "pre_submit_screenshot", "post_submit_screenshot":
			continue
		case "form_fields":
			// Field values are identity data. Names alone are not required by the
			// campaign result or event contracts, so omit the object entirely.
			continue
		case "error", "hint":
			result[key] = boundedRedacted(stringValue(value), profile, 500)
		case "url", "final_url":
			result[key] = boundedRedacted(stringValue(value), profile, 2048)
		default:
			result[key] = value
		}
	}
	return result
}

func sanitizeEvidenceMap(value any, profile *identity.Profile) map[string]any {
	evidence, ok := value.(map[string]any)
	if !ok {
		return nil
	}
	result := map[string]any{}
	if finalURL := boundedRedacted(stringValue(evidence["final_url"]), profile, 2048); finalURL != "" {
		result["final_url"] = finalURL
	}
	addEvidenceDigest(result, "page_text", []byte(stringValue(evidence["page_text"])))
	addEvidenceDigest(result, "pre_submit_screenshot", screenshotBytes(evidence["pre_submit_screenshot"]))
	addEvidenceDigest(result, "post_submit_screenshot", screenshotBytes(evidence["post_submit_screenshot"]))
	return result
}

func executeEmailRequest(
	ctx context.Context,
	store *eventstore.Store,
	requestID int64,
	brokerName string,
	payload map[string]any,
	templateID string,
	opts ExecuteOpts,
) (map[string]any, error) {
	channelEndpoint, _ := payload["endpoint"].(string)

	profile, identityHash, err := loadProfile(ctx, opts.ProfilePath)
	if err != nil {
		return nil, err
	}

	requiredFields := []string{"full_name", "email_addresses"}
	if rf, ok := payload["required_fields"].([]any); ok {
		requiredFields = toStrings(rf)
	} else if rf2, ok := payload["required_fields"].([]string); ok {
		requiredFields = rf2
	}
	missing := missingFields(profile, requiredFields)
	if len(missing) > 0 {
		return nil, &ErrIdentityProfile{msg: fmt.Sprintf(
			"Missing required identity fields: %s. Run 'symeraseme init-profile' to update your profile.",
			strings.Join(missing, ", "),
		)}
	}

	render := opts.Render
	if render == nil {
		render = defaultRenderer
	}
	rendered, err := render(templateID, profile, brokerName)
	if err != nil {
		return nil, err
	}

	subject := "Data Deletion Request — " + brokerName
	if opts.DryRun {
		return map[string]any{
			"success":    true,
			"dry_run":    true,
			"request_id": requestID,
			"to":         channelEndpoint,
			"subject":    subject,
			"body":       rendered,
		}, nil
	}

	if opts.Email == nil {
		return nil, ErrEmailSenderRequired
	}
	sendResult, err := opts.Email(ctx, channelEndpoint, subject, rendered)
	if err != nil {
		_, _, aerr := storeAppend(ctx, store, requestID, eventstore.EvtSendFailed, map[string]any{
			"error": safeError(err),
			"to":    channelEndpoint,
		})
		if aerr != nil {
			return nil, aerr
		}
		return nil, &ExecutionError{RequestID: requestID, Msg: err.Error()}
	}

	expectedDays := 30
	if v, ok := payload["expected_response_days"].(float64); ok && v > 0 {
		expectedDays = int(v)
	}
	_, _, err = storeAppend(ctx, store, requestID, eventstore.EvtSent, map[string]any{
		"to":                     channelEndpoint,
		"template":               templateID,
		"account":                opts.Account,
		"expected_response_days": expectedDays,
		"message_id":             sendResult["message_id"],
		"identity_snapshot_hash": identityHash,
	})
	if err != nil {
		return nil, err
	}
	return map[string]any{"success": true, "request_id": requestID, "result": sendResult}, nil
}

// loadProfile loads the identity profile for email execution.  Unlike the
// planning path, a missing profile is a hard error here (Python raises
// ProfileError with the init-profile hint).
func loadProfile(ctx context.Context, profilePath string) (*identity.Profile, string, error) {
	path := profilePath
	if path == "" {
		p, err := identity.DefaultProfilePath()
		if err != nil {
			return nil, "", err
		}
		path = p
	}
	profile, err := identity.LoadProfile(path)
	if err != nil {
		if errors.Is(err, identity.ErrProfileNotFound) {
			return nil, "", ErrIdentityProfileNotFound
		}
		return nil, "", &ErrIdentityProfile{msg: err.Error()}
	}
	return profile, identity.HashProfile(profile), nil
}

// loadWebFormProfile loads the profile used to redact executor output. A
// missing profile is valid for manual fallback and yields an empty hash.
func loadWebFormProfile(profilePath string) (*identity.Profile, string, error) {
	path := profilePath
	if path == "" {
		var err error
		path, err = identity.DefaultProfilePath()
		if err != nil {
			return nil, "", err
		}
	}
	profile, err := identity.LoadProfile(path)
	if errors.Is(err, identity.ErrProfileNotFound) {
		return nil, "", nil
	}
	if err != nil {
		return nil, "", &ErrIdentityProfile{msg: err.Error()}
	}
	return profile, identity.HashProfile(profile), nil
}

// storeAppend wraps Store.AppendAndProject with the standard source and
// "now" timestamp the Python port uses.
func storeAppend(ctx context.Context, store *eventstore.Store, requestID int64, et eventstore.EventType, payload map[string]any) (int64, eventstore.StateJSON, error) {
	return store.AppendAndProject(ctx, requestID, et, payload, eventstore.SrcSystem, time.Now().UTC())
}

// missingFields mirrors the Python loop that treats None/[]/{}/"" as missing.
func missingFields(p *identity.Profile, fields []string) []string {
	var missing []string
	for _, f := range fields {
		if profileFieldEmpty(p, f) {
			missing = append(missing, f)
		}
	}
	return missing
}

// profileFieldEmpty resolves a profile field name to its emptiness.
func profileFieldEmpty(p *identity.Profile, field string) bool {
	switch field {
	case "full_name":
		return p.FullName == ""
	case "email_addresses":
		return len(p.EmailAddresses) == 0
	case "address":
		return len(p.Addresses) == 0
	case "date_of_birth":
		return p.DateOfBirth == nil || *p.DateOfBirth == ""
	case "state":
		// any address with a state counts as present; mirrors Python
		// treating an Address list as present when any entry exists.
		return len(p.Addresses) == 0
	case "name_variants":
		return len(p.NameVariants) == 0
	case "phone_numbers":
		return len(p.PhoneNumbers) == 0
	case "jurisdictions":
		return len(p.Jurisdictions) == 0
	default:
		return true
	}
}

func toStrings(xs []any) []string {
	out := make([]string, 0, len(xs))
	for _, x := range xs {
		if s, ok := x.(string); ok {
			out = append(out, s)
		}
	}
	return out
}

func stringValue(value any) string {
	if text, ok := value.(string); ok {
		return text
	}
	return ""
}

func stringOr(value any, fallback string) string {
	if text := stringValue(value); text != "" {
		return text
	}
	return fallback
}

func intValue(value any) int {
	switch number := value.(type) {
	case int:
		return number
	case int64:
		return int(number)
	case float64:
		return int(number)
	default:
		return 0
	}
}

func stringMap(value any) map[string]string {
	out := map[string]string{}
	switch fields := value.(type) {
	case map[string]string:
		return fields
	case map[string]any:
		for key, item := range fields {
			if text, ok := item.(string); ok {
				out[key] = text
			}
		}
	}
	return out
}

// safeError mirrors safe_error_str (best-effort string of an error).
func safeError(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

// defaultRenderer is a placeholder until internal/templating lands
// (issue #716); it produces a deterministic, obviously-templated body so
// the execution flow remains testable without the renderer.
func defaultRenderer(templateID string, profile *identity.Profile, brokerName string) (string, error) {
	name := ""
	if profile != nil {
		name = profile.FullName
	}
	return fmt.Sprintf("[template %s — %s / %s]", templateID, brokerName, name), nil
}

func manualWebFormTaskResult(ctx context.Context, store *eventstore.Store, brokerName string, requestID int64) (map[string]any, error) {
	formURL := ""
	if events, err := store.GetEvents(ctx, requestID, 0); err == nil {
		for i := len(events) - 1; i >= 0 && formURL == ""; i-- {
			for _, key := range []string{"form_url", "endpoint", "url"} {
				if value, ok := events[i].Payload[key].(string); ok && value != "" {
					formURL = value
					break
				}
			}
		}
	}
	task, err := manualtasks.Create(ctx, store, manualtasks.CreateOpts{
		RequestID: &requestID, BrokerID: brokerName, BrokerName: brokerName,
		FormURL: formURL, Reason: "dynamic_form",
	})
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"success": false, "status": "manual_action_required", "reason": "dynamic_form",
		"task_id": task.ID, "broker_id": brokerName, "broker_name": brokerName,
		"url": formURL, "instructions": task.Instructions, "dry_run": false,
	}, nil
}

// pathSafety keeps filepath imported for config-path handling parity in
// later adapter ports.
var _ = filepath.Join
