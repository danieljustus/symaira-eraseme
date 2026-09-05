package campaign

import (
	"context"
	"encoding/base64"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

func stringPointer(value string) *string  { return &value }
func floatPointer(value float64) *float64 { return &value }

func TestFormSpecFromBrokerMapsSelectAndTimeout(t *testing.T) {
	profile := &identity.Profile{
		FullName:       "Jane Example",
		EmailAddresses: []string{"jane@example.test"},
		PhoneNumbers:   []string{"+49 123"},
		DateOfBirth:    stringPointer("1980-01-02"),
		Addresses:      []identity.Address{{Street: "Main Street 1", City: "Berlin", PostalCode: "10115", Country: "DE", State: stringPointer("BE")}},
	}
	broker := registry.Broker{ID: "example", Name: "Example"}
	channel := registry.Channel{
		Type: "web_form", URL: "https://example.test/optout",
		RequiredFields: []string{"email", "state"},
		FormSpec: &registry.FormSpec{TimeoutSeconds: floatPointer(2.5), Steps: []registry.FormStep{
			{Fill: map[string]string{"input[name=email]": "${email}", "input[name=name]": "${full_name}"}, Select: map[string]string{"select[name=country]": "${country}"}},
			{Click: "button[type=submit]"},
		}},
	}
	spec, err := FormSpecFromBroker(broker, channel, profile)
	if err != nil {
		t.Fatal(err)
	}
	if spec.Name != "example" || spec.StartURL != channel.URL || spec.Timeout.String() != "2.5s" || spec.Submit.CSS != "button[type=submit]" {
		t.Fatalf("converted spec = %#v", spec)
	}
	if len(spec.Fields) != 3 || spec.Fields[0].Name != "email" || spec.Fields[0].Value != "jane@example.test" || !spec.Fields[0].Required {
		t.Fatalf("converted fields = %#v", spec.Fields)
	}
	if spec.Fields[1].Name != "full_name" || spec.Fields[1].Value != "Jane Example" || spec.Fields[2].Name != "country" || spec.Fields[2].Value != "DE" {
		t.Fatalf("identity values = %#v", spec.Fields)
	}
	if _, err := ConvertBrokerFormSpec(broker, registry.Channel{Type: "email"}, profile); err == nil {
		t.Fatal("non-web-form channel accepted")
	}
}

func TestWebFormAdapterDryRunAndConfigurationErrors(t *testing.T) {
	broker := registry.Broker{ID: "broker", Name: "Broker", OptOut: []registry.Channel{{
		Type: "web_form", URL: "https://broker.test/form", FormSpec: &registry.FormSpec{Steps: []registry.FormStep{{Click: "#submit"}}},
	}}}
	adapter := NewWebFormAdapter([]registry.Broker{broker}, nil)
	result := adapter.Run(context.Background(), "broker", true)
	if result["success"] != true || result["dry_run"] != true || result["broker_id"] != "broker" || result["steps"] != 1 {
		t.Fatalf("dry run = %#v", result)
	}
	if _, ok := result["fields"]; ok {
		t.Fatalf("dry run exposed resolved identity fields: %#v", result)
	}
	missing := adapter.Run(context.Background(), "missing", true)
	if missing["success"] != false || missing["reason"] != "generic_error" {
		t.Fatalf("missing broker = %#v", missing)
	}
	result = adapter.Run(context.Background(), "broker", false)
	if result["success"] != false || result["status"] != "manual_action_required" || result["reason"] != "dynamic_form" {
		t.Fatalf("missing executor fallback = %#v", result)
	}
	if _, err := adapter.ConfirmLink(context.Background(), "missing", "https://example.test"); err == nil {
		t.Fatal("unknown broker accepted for confirmation")
	}
}

func TestWebFormResultHelpersAndReasonMapping(t *testing.T) {
	for _, tc := range []struct {
		code Code
		want string
	}{
		{CodeBlockedCaptcha, "captcha_failed"},
		{CodeBlockedBotwall, "generic_error"},
		{CodeNavigationTimeout, "timeout"},
		{CodeFieldNotFound, "unknown_field"},
		{CodeConfirmationFailed, "assertion_failed"},
		{CodeSuccess, "generic_error"},
	} {
		if got := reasonForCode(tc.code); got != tc.want {
			t.Errorf("reasonForCode(%q) = %q, want %q", tc.code, got, tc.want)
		}
	}
	result := &Result{Code: CodeSuccess, Evidence: &Evidence{FinalURL: "https://done.test", PageText: "done", PreSubmitScreenshot: []byte("pre"), PostSubmitScreenshot: []byte("post")}}
	mapped := formResultMap(result, nil)
	evidence, _ := mapped["evidence"].(map[string]any)
	if mapped["final_url"] != "https://done.test" || mapped["page_text"] != nil || evidence["page_text_sha256"] == nil || evidence["post_submit_screenshot_sha256"] == nil {
		t.Fatalf("mapped evidence = %#v", mapped)
	}
	post := base64.StdEncoding.EncodeToString([]byte("post"))
	if string(resultScreenshot(map[string]any{"evidence": map[string]any{"post_submit_screenshot": post}})) != "post" {
		t.Fatal("base64 screenshot was not decoded")
	}
	if resultScreenshot(map[string]any{}) != nil || screenshotBytes(42) != nil || screenshotBytes("not-base64") != nil {
		t.Fatal("invalid screenshot input accepted")
	}
	if requiredField("email", map[string]bool{"email_addresses": true}) != true || requiredField("full_name", map[string]bool{}) {
		t.Fatal("required field aliases failed")
	}
}

func TestIdentityFieldValuesHandlesNilAndMultipleAddresses(t *testing.T) {
	if values := identityFieldValues(nil); len(values) != 0 {
		t.Fatalf("nil profile values = %#v", values)
	}
	profile := &identity.Profile{FullName: "Single", Addresses: []identity.Address{{Street: "One", City: "A"}, {Street: "Two", City: "B"}}}
	values := identityFieldValues(profile)
	if values["first_name"] != "Single" || values["last_name"] != "" || values["address_city_1"] != "B" {
		t.Fatalf("multiple address values = %#v", values)
	}
	name, resolved := resolveIdentityValue("${missing}-${full_name}", values)
	if name != "missing" || resolved != "-Single" {
		t.Fatalf("identity placeholder = %q/%q", name, resolved)
	}
}
