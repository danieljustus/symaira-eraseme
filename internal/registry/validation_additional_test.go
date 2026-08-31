package registry

import "testing"

func TestValidateChannelRejectsInvalidContractVariants(t *testing.T) {
	validForm := &FormSpec{Steps: []FormStep{{Goto: "https://example.test"}}}
	cases := []struct {
		name    string
		channel Channel
	}{
		{"unknown type", Channel{Type: "phone"}},
		{"email missing endpoint", Channel{Type: "email"}},
		{"email invalid endpoint", Channel{Type: "email", Endpoint: "not-an-email"}},
		{"email with URL", Channel{Type: "email", Endpoint: "a@example.test", URL: "https://example.test"}},
		{"web form missing URL", Channel{Type: "web_form", FormSpec: validForm}},
		{"web form invalid URL", Channel{Type: "web_form", URL: "", FormSpec: validForm}},
		{"web form missing spec", Channel{Type: "web_form", URL: "https://example.test"}},
		{"web form endpoint", Channel{Type: "web_form", URL: "https://example.test", FormSpec: validForm, Endpoint: "a@example.test"}},
		{"invalid template", Channel{Type: "email", Endpoint: "a@example.test", Template: "unknown"}},
		{"invalid locale", Channel{Type: "email", Endpoint: "a@example.test", Locale: "EN_us"}},
		{"invalid required field", Channel{Type: "email", Endpoint: "a@example.test", RequiredFields: []string{"unknown"}}},
		{"invalid response days", Channel{Type: "email", Endpoint: "a@example.test", ExpectedResponseDays: intPointer(0)}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if err := validateChannel(&tc.channel); err == nil {
				t.Fatal("invalid channel accepted")
			}
		})
	}
}

func TestValidateFormStepCoversActionsAndSecurityConstraints(t *testing.T) {
	if err := validateFormSpec(&FormSpec{}); err == nil {
		t.Fatal("empty form spec accepted")
	}
	if err := validateFormStep(&FormStep{}); err == nil {
		t.Fatal("empty form step accepted")
	}
	negative := -1.0
	if err := validateFormStep(&FormStep{WaitSeconds: &negative}); err == nil {
		t.Fatal("negative wait accepted")
	}
	for _, captcha := range []*SolveCaptcha{
		{Type: "unknown", SiteKey: "12345678"},
		{Type: "turnstile", SiteKey: "short"},
		{Type: "turnstile", SiteKey: "12345678", Provider: "unknown"},
		{Type: "turnstile", SiteKey: "12345678", MinScore: floatPointer(2)},
	} {
		if err := validateFormStep(&FormStep{SolveCaptcha: captcha}); err == nil {
			t.Fatalf("invalid captcha accepted: %#v", captcha)
		}
	}
	if err := validateFormStep(&FormStep{Fill: map[string]string{"@": "x"}}); err == nil {
		t.Fatal("invalid fill selector accepted")
	}
	if err := validateFormStep(&FormStep{Select: map[string]string{"@": "x"}}); err == nil {
		t.Fatal("invalid select selector accepted")
	}
	if err := validateFormStep(&FormStep{Goto: "https://example.test", Fill: map[string]string{"input[name=email]": "x"}, Select: map[string]string{"select[name=country]": "DE"}, Click: "#submit", WaitFor: "done", Screenshot: "shot.png", AssertText: "success", SolveCaptcha: &SolveCaptcha{Type: "turnstile", SiteKey: "12345678"}}); err != nil {
		t.Fatal(err)
	}
}

func TestDecodeAndValidateRejectsRequiredTopLevelValues(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"
	for _, suffix := range []string{
		"",
		"jurisdictions: []\n",
		"laws: []\n",
		"priority: invalid\n",
		"status: invalid\n",
		"added_date: tomorrow\n",
	} {
		doc := &doc{id: "test", path: "test.yaml", content: []byte(base + suffix)}
		if suffix == "" {
			if _, err := decodeAndValidate(doc); err != nil {
				t.Fatal(err)
			}
			continue
		}
		if _, err := decodeAndValidate(doc); err == nil {
			t.Fatalf("invalid document accepted: %q", suffix)
		}
	}
}

func intPointer(value int) *int           { return &value }
func floatPointer(value float64) *float64 { return &value }
