package registry

import (
	"encoding/json"
	"fmt"
	"net/mail"
	"regexp"
	"strings"

	"gopkg.in/yaml.v3"
)

// ValidationError distinguishes contract violations from I/O errors.
type ValidationError struct{ msg string }

func (e *ValidationError) Error() string { return e.msg }

func verr(format string, args ...any) error {
	return &ValidationError{msg: fmt.Sprintf(format, args...)}
}

// Closed enums from the contract (docs/registry-contract.md §3–§6).
var (
	categories     = set("people-search", "marketing", "credit", "analytics", "background-check", "social-media", "other")
	jurisdictions  = set("AT", "CH", "DE", "DK", "EU", "FI", "FR", "GB", "IE", "IL", "LU", "NL", "NO", "SE", "UK", "US")
	lawsEnum       = set("GDPR", "CCPA", "CPRA", "LGPD", "PIPEDA")
	priorities     = set("high", "medium", "low")
	statuses       = set("active", "deprecated", "merged", "out-of-business")
	channelTypes   = set("email", "web_form")
	templates      = set("ccpa-deletion", "gdpr-art17")
	captchaTypes   = set("recaptcha-v2", "recaptcha-v3", "hcaptcha", "turnstile")
	captchaProv    = set("capsolver", "2captcha", "anticaptcha")
	requiredFields = set("full_name", "email", "address", "date_of_birth", "state")
)

var (
	idPattern     = regexp.MustCompile(`^[a-z0-9-]+$`)
	localePattern = regexp.MustCompile(`^[a-z]{2}(-[A-Z]{2})?$`)
	// selectorPrefix: contract §6 says keys must "look like CSS selectors".
	// Real data uses attribute selectors like input[name=x], so accept any
	// selector-ish token: must contain a tag/attr/id/class marker.
	selectorPrefix = regexp.MustCompile(`^[a-zA-Z\[#.*][a-zA-Z0-9\[\]=.'"_:#.\- *>,~+]*$`)
)

func set(items ...string) map[string]bool {
	m := make(map[string]bool, len(items))
	for _, s := range items {
		m[s] = true
	}
	return m
}

// Broker mirrors the Registry Data Contract §3. YAML keys are the contract
// keys; unknown top-level fields are rejected (decoding via strict map first).
type Broker struct {
	ID              string        `yaml:"id" json:"id"`
	Name            string        `yaml:"name" json:"name"`
	Website         string        `yaml:"website" json:"website"`
	Category        string        `yaml:"category" json:"category"`
	Jurisdictions   []string      `yaml:"jurisdictions" json:"jurisdictions"`
	Laws            []string      `yaml:"laws" json:"laws"`
	DataSensitivity *int          `yaml:"data_sensitivity" json:"data_sensitivity,omitempty"`
	Priority        string        `yaml:"priority" json:"priority"`
	OptOut          []Channel     `yaml:"opt_out" json:"opt_out"`
	Verification    *Verification `yaml:"verification" json:"verification,omitempty"`
	Disabled        *bool         `yaml:"disabled" json:"disabled,omitempty"`
	AddedDate       string        `yaml:"added_date" json:"added_date,omitempty"`
	Source          string        `yaml:"source" json:"source,omitempty"`
	Status          string        `yaml:"status" json:"status,omitempty"`
	Notes           string        `yaml:"notes" json:"notes,omitempty"`

	// Defaults applied after validation (authoritative per §9).
	DataSensitivityDefault int    `yaml:"-" json:"-"`
	StatusDefault          string `yaml:"-" json:"-"`
}

// Channel is one opt-out channel (contract §4).
type Channel struct {
	Type                 string    `yaml:"type" json:"type"`
	Endpoint             string    `yaml:"endpoint" json:"endpoint,omitempty"`
	URL                  string    `yaml:"url" json:"url,omitempty"`
	FormSpec             *FormSpec `yaml:"form_spec" json:"form_spec,omitempty"`
	Template             string    `yaml:"template" json:"template,omitempty"`
	Locale               string    `yaml:"locale" json:"locale,omitempty"`
	RequiredFields       []string  `yaml:"required_fields" json:"required_fields,omitempty"`
	SupportsSuppression  *bool     `yaml:"supports_suppression" json:"supports_suppression,omitempty"`
	ExpectedResponseDays *int      `yaml:"expected_response_days" json:"expected_response_days,omitempty"`
	Disabled             *bool     `yaml:"disabled" json:"disabled,omitempty"`
}

// Verification is the optional keyword block (contract §5).
type Verification struct {
	AckKeywords           []string `yaml:"ack_keywords" json:"ack_keywords,omitempty"`
	RejectionKeywords     []string `yaml:"rejection_keywords" json:"rejection_keywords,omitempty"`
	HumanRequiredKeywords []string `yaml:"human_required_keywords" json:"human_required_keywords,omitempty"`
}

// FormSpec is the web-form step DSL (contract §6).
type FormSpec struct {
	Steps          []FormStep `yaml:"steps" json:"steps"`
	TimeoutSeconds *float64   `yaml:"timeout_seconds" json:"timeout_seconds,omitempty"`
	RateLimitDelay *float64   `yaml:"rate_limit_delay" json:"rate_limit_delay,omitempty"`
	Headless       *bool      `yaml:"headless" json:"headless,omitempty"`
}

// FormStep is one step of the step DSL; all keys optional, at least one
// must be present.
type FormStep struct {
	Goto         string            `yaml:"goto" json:"goto,omitempty"`
	Fill         map[string]string `yaml:"fill" json:"fill,omitempty"`
	Select       map[string]string `yaml:"select" json:"select,omitempty"`
	Click        string            `yaml:"click" json:"click,omitempty"`
	WaitFor      string            `yaml:"wait_for" json:"wait_for,omitempty"`
	WaitSeconds  *float64          `yaml:"wait_seconds" json:"wait_seconds,omitempty"`
	Screenshot   string            `yaml:"screenshot" json:"screenshot,omitempty"`
	AssertText   string            `yaml:"assert_text" json:"assert_text,omitempty"`
	SolveCaptcha *SolveCaptcha     `yaml:"solve_captcha" json:"solve_captcha,omitempty"`
}

// SolveCaptcha describes a captcha-solving step (contract §6).
type SolveCaptcha struct {
	Type        string   `yaml:"type" json:"type"`
	SiteKey     string   `yaml:"site_key" json:"site_key"`
	Provider    string   `yaml:"provider" json:"provider,omitempty"`
	Action      string   `yaml:"action" json:"action,omitempty"`
	MinScore    *float64 `yaml:"min_score" json:"min_score,omitempty"`
	IsInvisible *bool    `yaml:"is_invisible" json:"is_invisible,omitempty"`
}

// brokerKeys is the closed top-level key whitelist (contract §3,
// additionalProperties: false).
var brokerKeys = set(
	"id", "name", "website", "category", "jurisdictions", "laws",
	"data_sensitivity", "priority", "opt_out", "verification", "disabled",
	"added_date", "source", "status", "notes",
)

// decodeAndValidate decodes a raw broker document and validates it against
// the contract. Returns ValidationError for contract violations. Unknown
// top-level, channel, verification, and form-spec fields are rejected.
func decodeAndValidate(d *doc) (Broker, error) {
	var raw map[string]yaml.Node
	dec := yaml.NewDecoder(strings.NewReader(string(d.content)))
	if err := dec.Decode(&raw); err != nil {
		return Broker{}, verr("yaml decode: %v", err)
	}
	var unknown []string
	for k := range raw {
		if !brokerKeys[k] {
			unknown = append(unknown, k)
		}
	}
	if len(unknown) > 0 {
		sortStrings(unknown)
		return Broker{}, verr("unknown top-level field(s): %s", strings.Join(unknown, ", "))
	}
	// Strict re-decode: KnownFields(true) rejects unknown fields at every
	// nesting level (channels, verification, form_spec, solve_captcha).
	strict := yaml.NewDecoder(strings.NewReader(string(d.content)))
	strict.KnownFields(true)
	var b Broker
	if err := strict.Decode(&b); err != nil {
		return Broker{}, verr("schema: %v", err)
	}

	// id: pattern + must equal file stem (§3).
	if b.ID != d.id {
		return Broker{}, verr("id %q does not equal file stem %q", b.ID, d.id)
	}
	if !idPattern.MatchString(b.ID) {
		return Broker{}, verr("id %q does not match ^[a-z0-9-]+$", b.ID)
	}
	if strings.TrimSpace(b.Name) == "" {
		return Broker{}, verr("name is required")
	}
	if err := validURI(b.Website); err != nil {
		return Broker{}, verr("website: %v", err)
	}
	if !categories[b.Category] {
		return Broker{}, verr("category %q is not in the closed enum", b.Category)
	}
	if len(b.Jurisdictions) == 0 {
		return Broker{}, verr("jurisdictions must have at least 1 entry")
	}
	for _, j := range b.Jurisdictions {
		if !jurisdictions[j] {
			return Broker{}, verr("jurisdiction %q is not in the closed enum", j)
		}
	}
	if len(b.Laws) == 0 {
		return Broker{}, verr("laws must have at least 1 entry")
	}
	for _, l := range b.Laws {
		if !lawsEnum[l] {
			return Broker{}, verr("law %q is not in the closed enum", l)
		}
	}
	if b.DataSensitivity != nil && (*b.DataSensitivity < 1 || *b.DataSensitivity > 5) {
		return Broker{}, verr("data_sensitivity %d out of range 1..5", *b.DataSensitivity)
	}
	if !priorities[b.Priority] {
		return Broker{}, verr("priority %q is not in the closed enum", b.Priority)
	}
	if len(b.OptOut) == 0 {
		return Broker{}, verr("opt_out must have at least 1 channel")
	}
	for i := range b.OptOut {
		if err := validateChannel(&b.OptOut[i]); err != nil {
			return Broker{}, verr("opt_out[%d]: %v", i, err)
		}
	}
	if b.Status != "" && !statuses[b.Status] {
		return Broker{}, verr("status %q is not in the closed enum", b.Status)
	}
	if b.AddedDate != "" && !datePattern.MatchString(b.AddedDate) {
		return Broker{}, verr("added_date %q is not ISO 8601 date", b.AddedDate)
	}

	// Defaults (§9): authoritative default values.
	if b.DataSensitivity == nil {
		three := 3
		b.DataSensitivity = &three
	}
	if b.Status == "" {
		b.Status = "active"
	}
	return b, nil
}

var datePattern = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}$`)

// validateChannel enforces contract §4: exactly one variant, closed enums,
// unknown channel fields rejected.
func validateChannel(c *Channel) error {
	if !channelTypes[c.Type] {
		return verr("channel type %q is not in the closed enum", c.Type)
	}
	switch c.Type {
	case "email":
		if c.Endpoint == "" {
			return verr("email channel requires endpoint")
		}
		if _, err := mail.ParseAddress(c.Endpoint); err != nil {
			return verr("endpoint %q is not a valid email", c.Endpoint)
		}
		if c.URL != "" || c.FormSpec != nil {
			return verr("email channel must not carry web_form fields (url/form_spec)")
		}
	case "web_form":
		if c.URL == "" {
			return verr("web_form channel requires url")
		}
		if err := validURI(c.URL); err != nil {
			return verr("url: %v", err)
		}
		if c.FormSpec == nil {
			return verr("web_form channel requires form_spec")
		}
		if c.Endpoint != "" {
			return verr("web_form channel must not carry email fields (endpoint)")
		}
		if err := validateFormSpec(c.FormSpec); err != nil {
			return err
		}
	}
	if c.Template != "" && !templates[c.Template] {
		return verr("template %q is not in the closed enum", c.Template)
	}
	if c.Locale != "" && !localePattern.MatchString(c.Locale) {
		return verr("locale %q does not match RFC 5646 pattern", c.Locale)
	}
	for _, f := range c.RequiredFields {
		if !requiredFields[f] {
			return verr("required_fields item %q is not in the closed enum", f)
		}
	}
	if c.ExpectedResponseDays != nil && *c.ExpectedResponseDays < 1 {
		return verr("expected_response_days must be >= 1")
	}
	return nil
}

// validateFormSpec enforces contract §6.
func validateFormSpec(f *FormSpec) error {
	if len(f.Steps) == 0 {
		return verr("form_spec requires at least 1 step")
	}
	for i := range f.Steps {
		if err := validateFormStep(&f.Steps[i]); err != nil {
			return verr("steps[%d]: %v", i, err)
		}
	}
	return nil
}

// validateFormStep enforces the step DSL rules: at least one key present,
// selector-shaped keys for fill/select, captcha constraints.
func validateFormStep(s *FormStep) error {
	present := 0
	if s.Goto != "" {
		present++
	}
	if len(s.Fill) > 0 {
		present++
	}
	if len(s.Select) > 0 {
		present++
	}
	if s.Click != "" {
		present++
	}
	if s.WaitFor != "" {
		present++
	}
	if s.WaitSeconds != nil {
		present++
		if *s.WaitSeconds < 0 {
			return verr("wait_seconds must be >= 0")
		}
	}
	if s.Screenshot != "" {
		present++
	}
	if s.AssertText != "" {
		present++
	}
	if s.SolveCaptcha != nil {
		present++
		if !captchaTypes[s.SolveCaptcha.Type] {
			return verr("solve_captcha.type %q is not in the closed enum", s.SolveCaptcha.Type)
		}
		if len(s.SolveCaptcha.SiteKey) < 8 {
			return verr("solve_captcha.site_key must be at least 8 chars")
		}
		if s.SolveCaptcha.Provider != "" && !captchaProv[s.SolveCaptcha.Provider] {
			return verr("solve_captcha.provider %q is not in the closed enum", s.SolveCaptcha.Provider)
		}
		if s.SolveCaptcha.MinScore != nil && (*s.SolveCaptcha.MinScore < 0 || *s.SolveCaptcha.MinScore > 1) {
			return verr("solve_captcha.min_score out of range 0..1")
		}
	}
	if present == 0 {
		return verr("step must contain at least 1 action")
	}
	for sel := range s.Fill {
		if !selectorPrefix.MatchString(sel) {
			return verr("fill key %q does not look like a CSS selector", sel)
		}
	}
	for sel := range s.Select {
		if !selectorPrefix.MatchString(sel) {
			return verr("select key %q does not look like a CSS selector", sel)
		}
	}
	return nil
}

// validURI checks a uri-format field. Per contract §7/§9 the JSON Schema
// `format` keyword is treated as an annotation (matching the Python
// checker, which does not assert formats either) — real registry data
// contains legacy values like `privacy@host` or combined URLs with spaces
// in url fields. Enforced: non-empty.
func validURI(s string) error {
	if s == "" {
		return verr("is required")
	}
	return nil
}

// sortStrings is a tiny local sort to avoid importing sort twice in one file.
func sortStrings(s []string) {
	for i := 1; i < len(s); i++ {
		for j := i; j > 0 && s[j] < s[j-1]; j-- {
			s[j], s[j-1] = s[j-1], s[j]
		}
	}
}

// ensure json import is used (contract fields may be marshalled for tests).
var _ = json.Marshal
