package campaign

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-browse/formflow"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// FormDriverFactory supplies the narrow browser driver used by formflow. The
// factory keeps browser/session creation outside EraseMe and makes the adapter
// fully testable with a fake driver; this package never performs a live broker
// submission on its own.
type FormDriverFactory func(context.Context, string) (formflow.Driver, error)

// WebFormAdapter is the EraseMe boundary around symaira-browse/formflow. It
// resolves a broker's registry definition, converts its declarative steps to a
// semantic-first formflow spec, and maps typed outcomes to the campaign
// execution result shape.
type WebFormAdapter struct {
	brokers       map[string]registry.Broker
	driverFactory FormDriverFactory
	driver        formflow.Driver
	Pacer         *formflow.Pacer
	ProfilePath   string
}

// NewWebFormAdapter creates an adapter backed by a driver factory. A factory
// is preferred for campaign execution because each request can receive its
// own browser page/session.
func NewWebFormAdapter(brokers []registry.Broker, factory FormDriverFactory) *WebFormAdapter {
	byID := make(map[string]registry.Broker, len(brokers))
	for _, broker := range brokers {
		byID[broker.ID] = broker
	}
	return &WebFormAdapter{
		brokers:       byID,
		driverFactory: factory,
		Pacer:         formflow.NewPacer(0),
	}
}

// NewWebFormAdapterWithDriver creates an adapter for one supplied driver. It
// is useful for deterministic tests and single-request callers.
func NewWebFormAdapterWithDriver(brokers []registry.Broker, driver formflow.Driver) *WebFormAdapter {
	adapter := NewWebFormAdapter(brokers, nil)
	adapter.driver = driver
	return adapter
}

// FormSpecFromBroker converts one registry web_form channel to formflow's
// public spec. Registry selectors remain CSS fallbacks, while values are
// resolved from semantic identity field names such as ${email}; required_fields
// controls the loud pre-submit missing-field guarantee.
func FormSpecFromBroker(broker registry.Broker, channel registry.Channel, profile *identity.Profile) (formflow.FormSpec, error) {
	if channel.Type != "web_form" || channel.FormSpec == nil {
		return formflow.FormSpec{}, errors.New("campaign: broker channel is not a web form")
	}
	fields := identityFieldValues(profile)
	required := make(map[string]bool, len(channel.RequiredFields))
	for _, name := range channel.RequiredFields {
		required[name] = true
	}

	spec := formflow.FormSpec{
		Name:     broker.ID,
		StartURL: channel.URL,
	}
	if channel.FormSpec.TimeoutSeconds != nil && *channel.FormSpec.TimeoutSeconds > 0 {
		spec.Timeout = time.Duration(*channel.FormSpec.TimeoutSeconds * float64(time.Second))
	}

	var submitCSS string
	fieldIndex := 0
	for _, step := range channel.FormSpec.Steps {
		for _, selector := range sortedKeys(step.Fill) {
			value := step.Fill[selector]
			name, resolved := resolveIdentityValue(value, fields)
			if name == "" {
				name = fmt.Sprintf("field-%d", fieldIndex)
			}
			spec.Fields = append(spec.Fields, formflow.Field{
				Name:     name,
				Selector: formflow.Selector{CSS: selector},
				Value:    resolved,
				Required: requiredField(name, required),
			})
			fieldIndex++
		}
		// formflow's narrow Driver intentionally exposes Fill rather than a
		// separate select operation. A select value is therefore represented
		// as another field fill; the live driver handles the selector.
		for _, selector := range sortedKeys(step.Select) {
			value := step.Select[selector]
			name, resolved := resolveIdentityValue(value, fields)
			if name == "" {
				name = fmt.Sprintf("field-%d", fieldIndex)
			}
			spec.Fields = append(spec.Fields, formflow.Field{
				Name:     name,
				Selector: formflow.Selector{CSS: selector},
				Value:    resolved,
				Required: requiredField(name, required),
			})
			fieldIndex++
		}
		if step.Click != "" {
			// The final click in the registry DSL is the contracted submit
			// control. Earlier clicks (for example cookie consent) are not
			// representable by formflow's deliberately narrow surface.
			submitCSS = step.Click
		}
	}
	if submitCSS != "" {
		spec.Submit = formflow.Selector{CSS: submitCSS}
	}
	return spec, nil
}

// ConvertBrokerFormSpec is a descriptive alias for callers that prefer an
// explicitly named conversion entry point.
func ConvertBrokerFormSpec(broker registry.Broker, channel registry.Channel, profile *identity.Profile) (formflow.FormSpec, error) {
	return FormSpecFromBroker(broker, channel, profile)
}

// SubmitForm executes the selected broker's web form and returns formflow's
// typed result. Page outcomes are returned in Result.Code; Go errors are
// reserved for missing registry/driver configuration.
func (a *WebFormAdapter) SubmitForm(ctx context.Context, brokerID string) (*formflow.Result, error) {
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
	driver, err := a.newDriver(ctx, brokerID)
	if err != nil {
		return nil, err
	}
	runner := formflow.NewRunner(driver)
	if a.Pacer != nil {
		runner.Pacer = a.Pacer
	}
	return runner.SubmitForm(ctx, spec)
}

// ConfirmLink executes a confirmation link with the same in-process driver
// boundary as form submission.
func (a *WebFormAdapter) ConfirmLink(ctx context.Context, brokerID, linkURL string) (*formflow.Result, error) {
	if _, ok := a.brokers[brokerID]; !ok {
		return nil, fmt.Errorf("campaign: broker %q not found", brokerID)
	}
	driver, err := a.newDriver(ctx, brokerID)
	if err != nil {
		return nil, err
	}
	runner := formflow.NewRunner(driver)
	if a.Pacer != nil {
		runner.Pacer = a.Pacer
	}
	return runner.ConfirmLink(ctx, formflow.ConfirmationSpec{LinkURL: linkURL})
}

// Run adapts SubmitForm to campaign.WebFormRunner. Evidence is intentionally
// kept as raw screenshot bytes here so eventstore JSON encoding stores them as
// base64 without losing the typed formflow data.
func (a *WebFormAdapter) Run(ctx context.Context, brokerID string, dryRun bool) map[string]any {
	broker, channel, err := a.webForm(brokerID)
	if err != nil {
		return map[string]any{"success": false, "code": string(formflow.CodeInvalidSpec), "error": err.Error(), "reason": "generic_error"}
	}
	profile, err := a.profile()
	if err != nil {
		return map[string]any{"success": false, "code": string(formflow.CodeInteractionFailed), "error": err.Error(), "reason": "generic_error"}
	}
	spec, err := FormSpecFromBroker(broker, channel, profile)
	if err != nil {
		return map[string]any{"success": false, "code": string(formflow.CodeInvalidSpec), "error": err.Error(), "reason": "generic_error"}
	}
	if dryRun {
		return map[string]any{
			"success":     true,
			"dry_run":     true,
			"broker_id":   broker.ID,
			"broker_name": broker.Name,
			"url":         spec.StartURL,
			"steps":       len(channel.FormSpec.Steps),
		}
	}
	result, err := a.SubmitForm(ctx, brokerID)
	if err != nil {
		return map[string]any{"success": false, "code": string(formflow.CodeInteractionFailed), "error": err.Error(), "reason": "generic_error", "url": spec.StartURL}
	}
	out := formResultMap(result)
	out["success"] = result.Code == formflow.CodeSuccess
	out["broker_id"] = broker.ID
	out["broker_name"] = broker.Name
	out["url"] = spec.StartURL
	out["reason"] = reasonForCode(result.Code)
	if result.Evidence != nil && result.Evidence.FinalURL != "" {
		out["url"] = result.Evidence.FinalURL
	}
	return out
}

// Confirm adapts ConfirmLink to the confirmation runner shape used by the
// event-store entry point.
func (a *WebFormAdapter) Confirm(ctx context.Context, brokerID, linkURL string) map[string]any {
	result, err := a.ConfirmLink(ctx, brokerID, linkURL)
	if err != nil {
		return map[string]any{"success": false, "code": string(formflow.CodeInteractionFailed), "error": err.Error(), "reason": "generic_error", "url": linkURL}
	}
	out := formResultMap(result)
	out["success"] = result.Code == formflow.CodeSuccess
	out["url"] = linkURL
	out["reason"] = reasonForCode(result.Code)
	if result.Evidence != nil && result.Evidence.FinalURL != "" {
		out["url"] = result.Evidence.FinalURL
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

func (a *WebFormAdapter) newDriver(ctx context.Context, brokerID string) (formflow.Driver, error) {
	if a.driver != nil {
		return a.driver, nil
	}
	if a.driverFactory == nil {
		return nil, errors.New("campaign: web form driver factory is required")
	}
	return a.driverFactory(ctx, brokerID)
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
		return &identity.Profile{}, nil
	}
	return profile, err
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
		values["first_name"] = parts[0]
		values["last_name"] = strings.Join(parts[1:], " ")
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
		values["address"] = address.Street
		values["state"] = ptrString(address.State)
		values["country"] = address.Country
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
	aliases := map[string][]string{
		"email":        {"email_addresses"},
		"phone_number": {"phone_numbers"},
	}
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

func formResultMap(result *formflow.Result) map[string]any {
	out := map[string]any{
		"code":           string(result.Code),
		"error":          result.Message,
		"step":           result.FailedStep,
		"failed_field":   result.FailedField,
		"hint":           result.Hint,
		"skipped_fields": result.Skipped,
		"duration_ms":    result.DurationMS,
	}
	if result.Evidence != nil {
		out["evidence"] = map[string]any{
			"final_url":              result.Evidence.FinalURL,
			"page_text":              result.Evidence.PageText,
			"pre_submit_screenshot":  result.Evidence.PreSubmitScreenshot,
			"post_submit_screenshot": result.Evidence.PostSubmitScreenshot,
		}
		out["final_url"] = result.Evidence.FinalURL
		out["page_text"] = result.Evidence.PageText
	}
	return out
}

func reasonForCode(code formflow.Code) string {
	switch code {
	case formflow.CodeBlockedCaptcha:
		return "captcha_failed"
	case formflow.CodeBlockedBotwall:
		return "generic_error"
	case formflow.CodeNavigationTimeout:
		return "timeout"
	case formflow.CodeFieldNotFound:
		return "unknown_field"
	case formflow.CodeConfirmationFailed:
		return "assertion_failed"
	default:
		return "generic_error"
	}
}

func reasonForCodeString(value any) string {
	code, _ := value.(string)
	return reasonForCode(formflow.Code(code))
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
