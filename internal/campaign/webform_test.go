package campaign

import (
	"testing"

	"github.com/danieljustus/symaira-browse/formflow"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

func TestFormSpecFromBrokerResolvesIdentityAndRequiredFields(t *testing.T) {
	broker := registry.Broker{ID: "acme", Name: "Acme", OptOut: []registry.Channel{{
		Type: "web_form", URL: "https://acme.example/optout", RequiredFields: []string{"email"},
		FormSpec: &registry.FormSpec{Steps: []registry.FormStep{{
			Fill:  map[string]string{"input[name=email]": "${email}"},
			Click: "button[type=submit]",
		}}},
	}}}
	profile := &identity.Profile{FullName: "Jane Doe", EmailAddresses: []string{"jane@example.com"}}
	spec, err := FormSpecFromBroker(broker, broker.OptOut[0], profile)
	if err != nil {
		t.Fatal(err)
	}
	if spec.StartURL != "https://acme.example/optout" || spec.Submit.CSS != "button[type=submit]" {
		t.Fatalf("unexpected spec: %+v", spec)
	}
	if len(spec.Fields) != 1 || spec.Fields[0].Value != "jane@example.com" || !spec.Fields[0].Required {
		t.Fatalf("unexpected fields: %+v", spec.Fields)
	}
}

func TestReasonForFormflowOutcome(t *testing.T) {
	cases := map[formflow.Code]string{
		formflow.CodeBlockedCaptcha:    "captcha_failed",
		formflow.CodeNavigationTimeout: "timeout",
		formflow.CodeFieldNotFound:     "unknown_field",
	}
	for code, want := range cases {
		if got := reasonForCode(code); got != want {
			t.Errorf("reasonForCode(%q) = %q, want %q", code, got, want)
		}
	}
}
