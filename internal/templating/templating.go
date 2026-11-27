// Package templating is the Go port of src/symeraseme/core/templating.py:
// Jinja2 letters (registry/laws/*.md.j2) and HTML reports
// (registry/templates/*.html.j2) rendered with Go's text/template.
//
// The template SOURCES are manually converted from Jinja2 to Go template
// syntax (issue #716 — "expect each template to need review, not sed")
// and embedded next to this package.  The golden conformance test renders
// every template with the identical fixture inputs the Python generator
// used and requires byte-identical output, so a conversion error in any
// letter surfaces as a test failure — this is where legal correctness of
// the product lives.
package templating

import (
	"embed"
	"fmt"
	"html"
	htmltemplate "html/template"
	"math"
	"sort"
	"strconv"
	"strings"
	texttemplate "text/template"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

//go:embed templates
var templateFS embed.FS

// Kind mirrors Jinja2's select_autoescape: markdown letters are rendered
// raw, HTML reports are auto-escaped (interpolations only — like Jinja).
type Kind int

const (
	KindMarkdown Kind = iota
	KindHTML
)

// nameToFile maps the public template name (the Python .j2 name, which is
// what execution.go passes as template_id) to the embedded Go source.
var nameToFile = map[string]string{
	"ccpa-deletion.en.md.j2":              "templates/laws/ccpa-deletion.en.md.gotmpl",
	"ccpa-opt-out.en.md.j2":               "templates/laws/ccpa-opt-out.en.md.gotmpl",
	"ccpa-rebuttal-deletion.md.j2":        "templates/laws/ccpa-rebuttal-deletion.md.gotmpl",
	"gdpr-art17.de.md.j2":                 "templates/laws/gdpr-art17.de.md.gotmpl",
	"gdpr-art17.en.md.j2":                 "templates/laws/gdpr-art17.en.md.gotmpl",
	"gdpr-rebuttal-address.md.j2":         "templates/laws/gdpr-rebuttal-address.md.gotmpl",
	"gdpr-rebuttal-identity.md.j2":        "templates/laws/gdpr-rebuttal-identity.md.gotmpl",
	"gdpr-rebuttal-rejected.en.md.j2":     "templates/laws/gdpr-rebuttal-rejected.en.md.gotmpl",
	"gdpr-rebuttal-verification.en.md.j2": "templates/laws/gdpr-rebuttal-verification.en.md.gotmpl",
	"dashboard.html.j2":                   "templates/dashboard.html.gotmpl",
	"report.html.j2":                      "templates/report.html.gotmpl",
}

// ListTemplateNames returns the public template names (sorted), mirroring
// Python list_templates() (which globs *.md.j2) plus the HTML reports.
func ListTemplateNames() []string {
	names := make([]string, 0, len(nameToFile))
	for n := range nameToFile {
		names = append(names, n)
	}
	sort.Strings(names)
	return names
}

// profileVars mirrors _profile_vars() in Python templating.py — a flat map
// with the same key names so the converted templates match the Jinja
// variable names.
func profileVars(p *identity.Profile) map[string]any {
	v := map[string]any{}
	if p == nil {
		return v
	}
	v["full_name"] = p.FullName
	v["name_variants"] = p.NameVariants
	dob := ""
	if p.DateOfBirth != nil {
		dob = *p.DateOfBirth
	}
	v["date_of_birth"] = dob
	addrs := make([]map[string]any, 0, len(p.Addresses))
	for _, a := range p.Addresses {
		addrs = append(addrs, map[string]any{
			"street":      a.Street,
			"city":        a.City,
			"postal_code": a.PostalCode,
			"country":     a.Country,
		})
	}
	v["addresses"] = addrs
	v["email_addresses"] = p.EmailAddresses
	v["phone_numbers"] = p.PhoneNumbers
	v["jurisdictions"] = p.Jurisdictions
	return v
}

// RenderOpts mirrors render_template() arguments.
type RenderOpts struct {
	Profile       *identity.Profile
	BrokerName    string
	BrokerWebsite string
	Brokers       []map[string]string
	ExtraVars     map[string]any
	Kind          Kind
}

// Render renders the named template.  Markdown letters parse with
// text/template, HTML reports with html/template (Jinja-equivalent
// autoescaping of interpolated values only).  Templates are parsed per
// call — they are tiny and caching adds state without value in a CLI.
// Both bare names ("gdpr-art17.en.md.j2") and golden-fixture names
// ("laws/gdpr-art17.en.md.j2") are accepted.
func Render(templateName string, opts RenderOpts) (string, error) {
	if strings.HasPrefix(templateName, "laws/") || strings.HasPrefix(templateName, "templates/") {
		templateName = strings.TrimPrefix(strings.TrimPrefix(templateName, "laws/"), "templates/")
	}
	file, ok := nameToFile[templateName]
	if !ok {
		return "", fmt.Errorf("templating: unknown template %q", templateName)
	}
	src, err := templateFS.ReadFile(file)
	if err != nil {
		return "", fmt.Errorf("templating: read %s: %w", file, err)
	}

	kind := opts.Kind
	if strings.HasSuffix(file, ".html.gotmpl") {
		kind = KindHTML
	} else if strings.HasSuffix(file, ".md.gotmpl") {
		kind = KindMarkdown
	}

	vars := map[string]any{}
	for k, val := range profileVars(opts.Profile) {
		vars[k] = val
	}
	vars["broker_name"] = opts.BrokerName
	vars["broker_website"] = opts.BrokerWebsite
	vars["brokers"] = opts.Brokers
	for k, val := range opts.ExtraVars {
		vars[k] = val
	}

	var sb strings.Builder
	if kind == KindHTML {
		// html/template escapes interpolated values contextually —
		// equivalent to Jinja's autoescape for .html.j2 (static markup is
		// not escaped, interpolated values are).
		fm := htmltemplate.FuncMap{}
		for k, v := range funcs() {
			fm[k] = v
		}
		fm["htmlcomment"] = func(v string) htmltemplate.HTML { //nolint:gosec -- static embedded template text
			return htmltemplate.HTML(v)
		}
		fm["csscomment"] = func(v string) htmltemplate.CSS { //nolint:gosec -- static embedded template text
			return htmltemplate.CSS(v)
		}
		fm["htmltext"] = func(v any) htmltemplate.HTML { //nolint:gosec -- escaped before marking safe
			return htmltemplate.HTML(html.EscapeString(fmt.Sprint(v)))
		}
		tmpl, err := htmltemplate.New(templateName).Funcs(fm).Parse(protectHTMLComments(string(src)))
		if err != nil {
			return "", fmt.Errorf("templating: parse %s: %w", templateName, err)
		}
		if err := tmpl.Execute(&sb, vars); err != nil {
			return "", fmt.Errorf("templating: execute %s: %w", templateName, err)
		}
	} else {
		// text/template: letters must NOT be escaped (Jinja's
		// select_autoescape leaves .md.j2 untouched).
		tmpl, err := texttemplate.New(templateName).Funcs(funcs()).Parse(string(src))
		if err != nil {
			return "", fmt.Errorf("templating: parse %s: %w", templateName, err)
		}
		if err := tmpl.Execute(&sb, vars); err != nil {
			return "", fmt.Errorf("templating: execute %s: %w", templateName, err)
		}
	}
	return sb.String(), nil
}

// protectHTMLComments preserves static HTML comments. Go's html/template
// deliberately strips literal comments, while Jinja2 keeps them in rendered
// output. Turning only embedded static comments into a safe helper call keeps
// byte parity without weakening escaping for interpolated user data.
func protectHTMLComments(src string) string {
	src = protectDelimitedComments(src, "<!--", "-->", "htmlcomment")
	return protectDelimitedComments(src, "/*", "*/", "csscomment")
}

func protectDelimitedComments(src, open, close, helper string) string {
	var out strings.Builder
	for {
		start := strings.Index(src, open)
		if start < 0 {
			out.WriteString(src)
			return out.String()
		}
		endRel := strings.Index(src[start+len(open):], close)
		if endRel < 0 {
			out.WriteString(src)
			return out.String()
		}
		end := start + len(open) + endRel + len(close)
		out.WriteString(src[:start])
		out.WriteString("{{")
		out.WriteString(helper)
		out.WriteByte(' ')
		out.WriteString(strconv.Quote(src[start:end]))
		out.WriteString("}}")
		src = src[end:]
	}
}

// funcs returns the template function map implementing the Jinja2 subset
// used by the converted templates.  All value helpers accept any and
// coerce numerically so map-backed data (data.planned etc.) works the way
// Jinja's dynamic dict attributes do.
func funcs() texttemplate.FuncMap {
	return texttemplate.FuncMap{
		"first":      first,
		"default":    defaultVal,
		"join":       join,
		"len":        length,
		"add":        add,
		"ge":         cmpGE,
		"le":         cmpLE,
		"gt":         cmpGT,
		"lt":         cmpLT,
		"eq":         cmpEQ,
		"not":        notFunc,
		"slice":      sliceStr,
		"slicelist":  sliceList,
		"replace":    replaceStr,
		"round":      roundF,
		"intF":       intF,
		"contains":   contains,
		"ternary":    ternary,
		"datefmt":    dateFmt,
		"div":        divF,
		"mul":        mulF,
		"sub":        subF,
		"mkstatuses": mkstatuses,
		"maxcount":   maxcount,
	}
}

// --- Jinja2 subset helpers -------------------------------------------

func first(v any) any {
	switch x := v.(type) {
	case []string:
		if len(x) > 0 {
			return x[0]
		}
	case []any:
		if len(x) > 0 {
			return x[0]
		}
	case []map[string]any:
		if len(x) > 0 {
			return x[0]
		}
	case string:
		if x != "" {
			return x[:1]
		}
	}
	return ""
}

// defaultVal mirrors the Jinja2 "default"/"or" fallback for falsy values.
func defaultVal(v, d any) any {
	switch x := v.(type) {
	case nil:
		return d
	case string:
		if x == "" {
			return d
		}
	case []string:
		if len(x) == 0 {
			return d
		}
	case []any:
		if len(x) == 0 {
			return d
		}
	case int:
		if x == 0 {
			return d
		}
	case float64:
		if x == 0 {
			return d
		}
	}
	return v
}

func join(v any, sep string) string {
	switch x := v.(type) {
	case []string:
		return strings.Join(x, sep)
	case []any:
		parts := make([]string, 0, len(x))
		for _, item := range x {
			parts = append(parts, fmt.Sprint(item))
		}
		return strings.Join(parts, sep)
	}
	return ""
}

func length(v any) int {
	switch x := v.(type) {
	case []string:
		return len(x)
	case []any:
		return len(x)
	case []map[string]any:
		return len(x)
	case map[string]any:
		return len(x)
	case string:
		return len(x)
	}
	return 0
}

func numF(v any) float64 {
	switch x := v.(type) {
	case int:
		return float64(x)
	case int64:
		return float64(x)
	case float64:
		return x
	case float32:
		return float64(x)
	case bool:
		if x {
			return 1
		}
		return 0
	case string:
		f, _ := strconv.ParseFloat(x, 64)
		return f
	}
	return 0
}

func add(vals ...any) int {
	sum := 0.0
	for _, v := range vals {
		sum += numF(v)
	}
	return int(sum)
}

func subF(a, b any) float64 { return numF(a) - numF(b) }
func divF(a, b any) float64 { return numF(a) / numF(b) }
func mulF(a, b any) float64 { return numF(a) * numF(b) }
func cmpGE(a, b any) bool   { return numF(a) >= numF(b) }
func cmpLE(a, b any) bool   { return numF(a) <= numF(b) }
func cmpGT(a, b any) bool   { return numF(a) > numF(b) }
func cmpLT(a, b any) bool   { return numF(a) < numF(b) }
func cmpEQ(a, b any) bool   { return fmt.Sprint(a) == fmt.Sprint(b) }
func notFunc(a bool) bool   { return !a }

// sliceStr mirrors Python string slicing [:n] (used for timestamps).
func sliceStr(s any, start, end int) string {
	runes := []rune(fmt.Sprint(s))
	if start < 0 {
		start = 0
	}
	if end < 0 || end > len(runes) {
		end = len(runes)
	}
	if start > end {
		return ""
	}
	return string(runes[start:end])
}

// sliceList mirrors Python list slicing [:n] (camp.requests[:100]).
func sliceList(v any, start, end int) []any {
	switch x := v.(type) {
	case []any:
		if start < 0 {
			start = 0
		}
		if end < 0 || end > len(x) {
			end = len(x)
		}
		if start > end {
			return []any{}
		}
		return x[start:end]
	case []map[string]any:
		if start < 0 {
			start = 0
		}
		if end < 0 || end > len(x) {
			end = len(x)
		}
		if start > end {
			return []any{}
		}
		out := make([]any, 0, end-start)
		for _, m := range x[start:end] {
			out = append(out, m)
		}
		return out
	}
	return []any{}
}

func replaceStr(s, old, new any) string {
	return strings.ReplaceAll(fmt.Sprint(s), fmt.Sprint(old), fmt.Sprint(new))
}

func roundF(v any) float64 { return math.Round(numF(v)) }

func intF(v any) int { return int(numF(v)) }

func contains(haystack, needle any) bool {
	return strings.Contains(fmt.Sprint(haystack), fmt.Sprint(needle))
}

// ternary mirrors Jinja2's inline "a if cond else b".
func ternary(cond bool, a, b any) any {
	if cond {
		return a
	}
	return b
}

// dateFmt mirrors now.strftime("%Y-%m-%d %H:%M UTC").
func dateFmt(t any, _ string) string {
	tt, ok := t.(time.Time)
	if !ok {
		return fmt.Sprint(t)
	}
	return tt.UTC().Format("2006-01-02 15:04 UTC")
}

// statusRow is the {"label", "count", "cls"} triple the dashboard chart
// consumes (the Jinja statuses list uses the same shape, attribute=1 is
// the count).
type statusRow struct {
	Label string
	Count int
	Cls   string
}

// mkstatuses mirrors the dashboard's {% set statuses = [...] %}.
func mkstatuses(data map[string]any) []statusRow {
	return []statusRow{
		{"Planned", intF(data["planned"]), "planned"},
		{"Sent", intF(data["sent"]), "sent"},
		{"Awaiting Ack", intF(data["awaiting_ack"]), "awaiting"},
		{"Awaiting Resp.", intF(data["awaiting_response"]), "awaiting"},
		{"Confirmed", intF(data["confirmed"]), "confirmed"},
		{"Rejected", intF(data["rejected"]), "rejected"},
		{"Overdue", intF(data["overdue"]), "overdue"},
	}
}

// maxcount mirrors statuses|map(attribute=1)|max or 1.
func maxcount(rows []statusRow) int {
	max := 0
	for _, r := range rows {
		if r.Count > max {
			max = r.Count
		}
	}
	if max == 0 {
		return 1
	}
	return max
}
