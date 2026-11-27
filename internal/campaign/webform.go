package campaign

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"fmt"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// Selector is the language-neutral selector contract shared by registry
// previews and injected test executors. It intentionally has no browser
// implementation or dependency on a sibling browser project.
type Selector struct {
	CSS string `json:"css,omitempty"`
}

// Field is one declarative form value.
type Field struct {
	Name     string   `json:"name"`
	Selector Selector `json:"selector"`
	Value    string   `json:"value"`
	Required bool     `json:"required"`
}

// FormSpec is the local, serializable web-form preview contract.
type FormSpec struct {
	Name     string        `json:"name"`
	StartURL string        `json:"start_url"`
	Timeout  time.Duration `json:"-"`
	Fields   []Field       `json:"fields"`
	Submit   Selector      `json:"submit"`
}

const defaultFormExecutorTimeout = 60 * time.Second

// ConfirmationSpec is the local confirmation-link contract.
type ConfirmationSpec struct {
	LinkURL string `json:"link_url"`
}

// Code is a stable result code returned by an injected executor.
type Code string

const (
	CodeSuccess            Code = "success"
	CodeInteractionFailed  Code = "interaction_failed"
	CodeInvalidSpec        Code = "invalid_spec"
	CodeBlockedCaptcha     Code = "blocked_captcha"
	CodeBlockedBotwall     Code = "blocked_botwall"
	CodeNavigationTimeout  Code = "navigation_timeout"
	CodeFieldNotFound      Code = "field_not_found"
	CodeConfirmationFailed Code = "confirmation_failed"
)

// Evidence contains executor-produced, non-sensitive evidence. Screenshot
// bytes are encoded by the normal JSON encoder when persisted in an event.
type Evidence struct {
	FinalURL             string `json:"final_url"`
	PageText             string `json:"page_text"`
	PreSubmitScreenshot  []byte `json:"pre_submit_screenshot,omitempty"`
	PostSubmitScreenshot []byte `json:"post_submit_screenshot,omitempty"`
}

// Result is the deterministic result contract used by injected executors.
type Result struct {
	Code        Code
	Message     string
	FailedStep  string
	FailedField string
	Hint        string
	Skipped     []string
	DurationMS  int64
	Evidence    *Evidence
}

// FormExecutor is the only side-effect boundary for web forms. Production
// does not provide one; tests may inject a deterministic implementation.
type FormExecutor interface {
	SubmitForm(context.Context, FormSpec) (*Result, error)
	ConfirmLink(context.Context, ConfirmationSpec) (*Result, error)
}

type FormExecutorFuncs struct {
	Submit  func(context.Context, FormSpec) (*Result, error)
	Confirm func(context.Context, ConfirmationSpec) (*Result, error)
}

func (f FormExecutorFuncs) SubmitForm(ctx context.Context, spec FormSpec) (*Result, error) {
	if f.Submit == nil {
		return nil, errors.New("campaign: form executor submit function is required")
	}
	return f.Submit(ctx, spec)
}

func (f FormExecutorFuncs) ConfirmLink(ctx context.Context, spec ConfirmationSpec) (*Result, error) {
	if f.Confirm == nil {
		return nil, errors.New("campaign: form executor confirmation function is required")
	}
	return f.Confirm(ctx, spec)
}

// WebFormAdapter is the EraseMe boundary around the local form contract. It
// previews registry forms and persists an explicit manual task when no
// executor is configured.
type WebFormAdapter struct {
	brokers   map[string]registry.Broker
	executor  FormExecutor
	Store     *eventstore.Store
	RequestID int64
	// DeferManualTask leaves task creation to executeWebformRequest, which has
	// the concrete request ID for each item in a campaign batch.
	DeferManualTask bool
	ProfilePath     string
}

func NewWebFormAdapter(brokers []registry.Broker, executor FormExecutor) *WebFormAdapter {
	byID := make(map[string]registry.Broker, len(brokers))
	for _, broker := range brokers {
		byID[broker.ID] = broker
	}
	return &WebFormAdapter{brokers: byID, executor: executor}
}

func NewWebFormAdapterWithStore(store *eventstore.Store, brokers []registry.Broker, executor FormExecutor) *WebFormAdapter {
	adapter := NewWebFormAdapter(brokers, executor)
	adapter.Store = store
	return adapter
}

func FormSpecFromBroker(broker registry.Broker, channel registry.Channel, profile *identity.Profile) (FormSpec, error) {
	if channel.Type != "web_form" || channel.FormSpec == nil {
		return FormSpec{}, errors.New("campaign: broker channel is not a web form")
	}
	fields := identityFieldValues(profile)
	required := make(map[string]bool, len(channel.RequiredFields))
	for _, name := range channel.RequiredFields {
		required[name] = true
	}
	spec := FormSpec{Name: broker.ID, StartURL: channel.URL}
	if channel.FormSpec.TimeoutSeconds != nil && *channel.FormSpec.TimeoutSeconds > 0 {
		spec.Timeout = time.Duration(*channel.FormSpec.TimeoutSeconds * float64(time.Second))
	}
	var submitCSS string
	fieldIndex := 0
	for _, step := range channel.FormSpec.Steps {
		for _, selector := range sortedKeys(step.Fill) {
			name, resolved := resolveIdentityValue(step.Fill[selector], fields)
			if name == "" {
				name = fmt.Sprintf("field-%d", fieldIndex)
			}
			spec.Fields = append(spec.Fields, Field{Name: name, Selector: Selector{CSS: selector}, Value: resolved, Required: requiredField(name, required)})
			fieldIndex++
		}
		for _, selector := range sortedKeys(step.Select) {
			name, resolved := resolveIdentityValue(step.Select[selector], fields)
			if name == "" {
				name = fmt.Sprintf("field-%d", fieldIndex)
			}
			spec.Fields = append(spec.Fields, Field{Name: name, Selector: Selector{CSS: selector}, Value: resolved, Required: requiredField(name, required)})
			fieldIndex++
		}
		if step.Click != "" {
			submitCSS = step.Click
		}
	}
	if submitCSS != "" {
		spec.Submit = Selector{CSS: submitCSS}
	}
	return spec, nil
}

func ConvertBrokerFormSpec(broker registry.Broker, channel registry.Channel, profile *identity.Profile) (FormSpec, error) {
	return FormSpecFromBroker(broker, channel, profile)
}

func (a *WebFormAdapter) SubmitForm(ctx context.Context, brokerID string) (*Result, error) {
	broker, channel, err := a.webForm(brokerID)
	if err != nil {
		return nil, err
	}
	profile, err := a.profile()
	if err != nil {
		return nil, err
	}
	spec, err := FormSpecFromBroker(broker, channel, profile)
	if err != nil {
		return nil, err
	}
	if a.executor == nil {
		return nil, errors.New("campaign: web form executor is not configured")
	}
	return callWithTimeout(ctx, spec.Timeout, func(callCtx context.Context) (*Result, error) {
		return a.executor.SubmitForm(callCtx, spec)
	})
}

func (a *WebFormAdapter) ConfirmLink(ctx context.Context, brokerID, linkURL string) (*Result, error) {
	if _, ok := a.brokers[brokerID]; !ok {
		return nil, fmt.Errorf("campaign: broker %q not found", brokerID)
	}
	if a.executor == nil {
		return nil, errors.New("campaign: web form executor is not configured")
	}
	return callWithTimeout(ctx, 0, func(callCtx context.Context) (*Result, error) {
		return a.executor.ConfirmLink(callCtx, ConfirmationSpec{LinkURL: linkURL})
	})
}

// Run returns a preview in dry-run mode. Without an injected executor it
// persists a durable manual task rather than claiming that a form succeeded.
func (a *WebFormAdapter) Run(ctx context.Context, brokerID string, dryRun bool) map[string]any {
	broker, channel, err := a.webForm(brokerID)
	if err != nil {
		return map[string]any{"success": false, "code": string(CodeInvalidSpec), "error": err.Error(), "reason": "generic_error", "dry_run": dryRun}
	}
	profile, err := a.profile()
	if err != nil {
		return map[string]any{"success": false, "code": string(CodeInteractionFailed), "error": err.Error(), "reason": "generic_error", "dry_run": dryRun}
	}
	spec, err := FormSpecFromBroker(broker, channel, profile)
	if err != nil {
		return map[string]any{"success": false, "code": string(CodeInvalidSpec), "error": err.Error(), "reason": "generic_error", "dry_run": dryRun}
	}
	if dryRun {
		return map[string]any{"success": true, "dry_run": true, "broker_id": broker.ID, "broker_name": broker.Name, "url": spec.StartURL, "steps": len(channel.FormSpec.Steps)}
	}
	if a.executor == nil {
		return a.createManualTask(ctx, broker, spec, "dynamic_form", profile)
	}
	result, err := a.SubmitForm(ctx, brokerID)
	if err != nil {
		reason := "generic_error"
		code := CodeInteractionFailed
		if errors.Is(err, context.DeadlineExceeded) {
			reason, code = "timeout", CodeNavigationTimeout
		}
		out := a.createManualTask(ctx, broker, spec, reason, profile)
		out["code"], out["error"] = string(code), boundedRedacted(err.Error(), profile, 500)
		return out
	}
	if result == nil {
		out := a.createManualTask(ctx, broker, spec, "generic_error", profile)
		out["code"], out["error"] = string(CodeInteractionFailed), "web form executor returned no result"
		return out
	}
	out := formResultMap(result, profile)
	out["success"] = result.Code == CodeSuccess
	out["dry_run"] = false
	out["broker_id"], out["broker_name"], out["url"] = broker.ID, broker.Name, spec.StartURL
	out["reason"] = reasonForCode(result.Code)
	if result.Evidence != nil && result.Evidence.FinalURL != "" {
		out["url"] = boundedRedacted(result.Evidence.FinalURL, profile, 2048)
	}
	if result.Code != CodeSuccess {
		manual := a.createManualTask(ctx, broker, spec, reasonForCode(result.Code), profile)
		for key, value := range out {
			manual[key] = value
		}
		manual["status"] = "manual_action_required"
		return manual
	}
	return out
}

func (a *WebFormAdapter) createManualTask(ctx context.Context, broker registry.Broker, spec FormSpec, reason string, profile *identity.Profile) map[string]any {
	result := map[string]any{"success": false, "status": "manual_action_required", "reason": reason, "broker_id": broker.ID, "broker_name": broker.Name, "url": spec.StartURL, "dry_run": false}
	if a.DeferManualTask {
		return result
	}
	if a.Store == nil {
		result["error"] = "campaign: manual task store is not configured"
		return result
	}
	var requestID *int64
	if a.RequestID != 0 {
		requestID = &a.RequestID
	}
	task, err := manualtasks.Create(ctx, a.Store, manualtasks.CreateOpts{RequestID: requestID, BrokerID: broker.ID, BrokerName: broker.Name, FormURL: spec.StartURL, Reason: reason, Profile: profile})
	if err != nil {
		result["error"] = err.Error()
		return result
	}
	result["task_id"], result["instructions"] = task.ID, task.Instructions
	return result
}

func (a *WebFormAdapter) Confirm(ctx context.Context, brokerID, linkURL string) map[string]any {
	result, err := a.ConfirmLink(ctx, brokerID, linkURL)
	if err != nil {
		return map[string]any{"success": false, "code": string(CodeInteractionFailed), "error": err.Error(), "reason": "generic_error", "url": linkURL}
	}
	if result == nil {
		return map[string]any{"success": false, "code": string(CodeInteractionFailed), "error": "web form executor returned no result", "reason": "generic_error", "url": linkURL}
	}
	profile, _ := a.profile()
	out := formResultMap(result, profile)
	out["success"], out["url"], out["reason"] = result.Code == CodeSuccess, boundedRedacted(linkURL, profile, 2048), reasonForCode(result.Code)
	if result.Evidence != nil && result.Evidence.FinalURL != "" {
		out["url"] = boundedRedacted(result.Evidence.FinalURL, profile, 2048)
	}
	return out
}

func (a *WebFormAdapter) webForm(brokerID string) (registry.Broker, registry.Channel, error) {
	broker, ok := a.brokers[brokerID]
	if !ok {
		return registry.Broker{}, registry.Channel{}, fmt.Errorf("campaign: broker %q not found", brokerID)
	}
	for _, channel := range broker.OptOut {
		if channel.Type == "web_form" && (channel.Disabled == nil || !*channel.Disabled) {
			return broker, channel, nil
		}
	}
	return registry.Broker{}, registry.Channel{}, fmt.Errorf("campaign: broker %q has no active web form channel", brokerID)
}

func (a *WebFormAdapter) profile() (*identity.Profile, error) {
	path := a.ProfilePath
	if path == "" {
		var err error
		path, err = identity.DefaultProfilePath()
		if err != nil {
			return nil, err
		}
	}
	profile, err := identity.LoadProfile(path)
	if errors.Is(err, identity.ErrProfileNotFound) {
		return nil, nil
	}
	return profile, err
}

func callWithTimeout(ctx context.Context, timeout time.Duration, fn func(context.Context) (*Result, error)) (*Result, error) {
	if timeout <= 0 {
		timeout = defaultFormExecutorTimeout
	}
	callCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	return fn(callCtx)
}

var identityPlaceholder = regexp.MustCompile(`\$\{([^}]+)\}`)

func identityFieldValues(profile *identity.Profile) map[string]string {
	values := map[string]string{}
	if profile == nil {
		return values
	}
	values["full_name"] = profile.FullName
	parts := strings.Fields(profile.FullName)
	if len(parts) > 0 {
		values["first_name"], values["last_name"] = parts[0], strings.Join(parts[1:], " ")
	}
	if len(profile.EmailAddresses) > 0 {
		values["email"] = profile.EmailAddresses[0]
	}
	if len(profile.PhoneNumbers) > 0 {
		values["phone_number"] = profile.PhoneNumbers[0]
	}
	if profile.DateOfBirth != nil {
		values["date_of_birth"] = *profile.DateOfBirth
	}
	if len(profile.Addresses) > 0 {
		address := profile.Addresses[0]
		values["address"], values["state"], values["country"] = address.Street, ptrString(address.State), address.Country
		for i, current := range profile.Addresses {
			values[fmt.Sprintf("address_street_%d", i)] = current.Street
			values[fmt.Sprintf("address_city_%d", i)] = current.City
			values[fmt.Sprintf("address_zip_%d", i)] = current.PostalCode
			values[fmt.Sprintf("address_state_%d", i)] = ptrString(current.State)
			values[fmt.Sprintf("address_country_%d", i)] = current.Country
		}
	}
	return values
}

func resolveIdentityValue(value string, fields map[string]string) (string, string) {
	name := ""
	resolved := identityPlaceholder.ReplaceAllStringFunc(value, func(match string) string {
		key := identityPlaceholder.FindStringSubmatch(match)[1]
		if name == "" {
			name = key
		}
		return fields[key]
	})
	return name, resolved
}

func requiredField(name string, required map[string]bool) bool {
	if required[name] {
		return true
	}
	aliases := map[string][]string{"email": {"email_addresses"}, "phone_number": {"phone_numbers"}}
	for _, alias := range aliases[name] {
		if required[alias] {
			return true
		}
	}
	return false
}

func sortedKeys(values map[string]string) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
func ptrString(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

func formResultMap(result *Result, profile *identity.Profile) map[string]any {
	if result == nil {
		return map[string]any{"code": string(CodeInteractionFailed), "error": "web form executor returned no result"}
	}
	out := map[string]any{
		"code": string(result.Code), "error": boundedRedacted(result.Message, profile, 500),
		"step": result.FailedStep, "failed_field": result.FailedField,
		"hint": boundedRedacted(result.Hint, profile, 500), "skipped_fields": result.Skipped,
		"duration_ms": result.DurationMS,
	}
	if result.Evidence != nil {
		evidence := map[string]any{}
		if finalURL := boundedRedacted(result.Evidence.FinalURL, profile, 2048); finalURL != "" {
			evidence["final_url"] = finalURL
			out["final_url"] = finalURL
		}
		addEvidenceDigest(evidence, "page_text", []byte(result.Evidence.PageText))
		addEvidenceDigest(evidence, "pre_submit_screenshot", result.Evidence.PreSubmitScreenshot)
		addEvidenceDigest(evidence, "post_submit_screenshot", result.Evidence.PostSubmitScreenshot)
		out["evidence"] = evidence
	}
	return out
}

func addEvidenceDigest(target map[string]any, name string, value []byte) {
	if len(value) == 0 {
		return
	}
	sum := sha256.Sum256(value)
	target[name+"_sha256"] = fmt.Sprintf("%x", sum[:])
	target[name+"_bytes"] = len(value)
}

func boundedRedacted(value string, profile *identity.Profile, limit int) string {
	redacted := manualtasks.RedactIdentityValues(value, profile)
	runes := []rune(redacted)
	if limit > 0 && len(runes) > limit {
		return string(runes[:limit])
	}
	return redacted
}

func reasonForCode(code Code) string {
	switch code {
	case CodeBlockedCaptcha:
		return "captcha_failed"
	case CodeNavigationTimeout:
		return "timeout"
	case CodeFieldNotFound:
		return "unknown_field"
	case CodeConfirmationFailed:
		return "assertion_failed"
	default:
		return "generic_error"
	}
}
func reasonForCodeString(value any) string {
	code, _ := value.(string)
	return reasonForCode(Code(code))
}
func resultScreenshot(result map[string]any) []byte {
	evidence, _ := result["evidence"].(map[string]any)
	if evidence == nil {
		return nil
	}
	if shot := screenshotBytes(evidence["post_submit_screenshot"]); len(shot) > 0 {
		return shot
	}
	return screenshotBytes(evidence["pre_submit_screenshot"])
}
func screenshotBytes(value any) []byte {
	switch data := value.(type) {
	case []byte:
		return data
	case string:
		decoded, err := base64.StdEncoding.DecodeString(data)
		if err == nil {
			return decoded
		}
	}
	return nil
}
