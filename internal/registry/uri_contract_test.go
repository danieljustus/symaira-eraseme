package registry

import (
	"path/filepath"
	"strings"
	"testing"
)

// validateYAML runs the loader's own decode-and-validate path over a document.
func validateYAML(t *testing.T, source string) error {
	t.Helper()
	_, err := decodeAndValidate(&doc{id: "test", path: "test.yaml", content: []byte(source)})
	return err
}

// The corpus used to contain 54 values that were not URIs: 46 email addresses
// recorded as `web_form` channels (#843) and 8 combined URL-plus-annotation
// strings. Because of them `validURI` accepted any non-empty string, so a
// `web_form` url reached the browser runner unchecked.
//
// These tests pin the strict rule that replaced it, including the scheme cases
// that must stay rejected: a browser navigation must not accept `javascript:`
// or `file:`.

func brokerWithWebsite(website string) string {
	return "id: test\nname: Test\nwebsite: " + website +
		"\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n" +
		"  - type: email\n    endpoint: a@example.test\n"
}

func brokerWithWebForm(url string) string {
	return "id: test\nname: Test\nwebsite: https://example.test" +
		"\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n" +
		"  - type: web_form\n    url: " + url +
		"\n    form_spec:\n      steps:\n        - goto: " + url + "\n"
}

func TestValidURIRejectsNonURIValues(t *testing.T) {
	for _, invalid := range []string{
		// The 46 email-as-url values from the legacy corpus.
		"privacy@www.comparethemarket.com",
		"privacy@addresssearch.com",
		// The combined URL-plus-annotation values.
		"https://cuebiq.com/privacy-center/ / privacy@cuebiq.com",
		"https://spydialer.com - Remove My Info link in footer",
		"https://rehold.com/ (Remove Button)",
		"https://tex warrant roundup",
		"https://www.gbg.com/en/privacy-policy/ / 0161 909 6713",
		// Schemes a browser must never be sent to.
		"javascript:alert(1)",
		"file:///etc/passwd",
		"data:text/html,<script>alert(1)</script>",
		"ftp://example.test/form",
		// Structurally broken http(s) values.
		"https://",
		"http:///path",
		"https://exa mple.test",
		"https://example.test/\nX",
		"",
	} {
		if err := validURI(invalid); err == nil {
			t.Errorf("validURI(%q) accepted a value the schema calls a URI", invalid)
		}
	}
}

func TestValidURIRejectsNonURIWebFormChannel(t *testing.T) {
	// The end-to-end path matters more than the unit: this is what a loader
	// failure looks like for the corpus values that were fixed.
	if err := validateYAML(t, brokerWithWebForm("privacy@host.example")); err == nil {
		t.Fatal("a web_form url that is not a URI must be rejected at load time")
	}
	for _, invalid := range []string{"javascript:alert(1)", "file:///etc/passwd", "https://exa mple.test"} {
		if err := validateYAML(t, brokerWithWebForm(invalid)); err == nil {
			t.Errorf("web_form url %q was accepted", invalid)
		}
	}
}

func TestValidURIRejectsNonURIWebsite(t *testing.T) {
	if err := validateYAML(t, brokerWithWebsite("privacy@example.test")); err == nil {
		t.Fatal("a website that is not a URI must be rejected")
	}
	if err := validateYAML(t, brokerWithWebsite("javascript:alert(1)")); err == nil {
		t.Fatal("a javascript: website must be rejected")
	}
}

func TestValidURIAcceptsTheCorpusShapes(t *testing.T) {
	// Every shape the cleaned corpus actually uses must stay loadable.
	for _, valid := range []string{
		"https://example.test",
		"https://example.test/",
		"https://example.test/path/to/form?x=1&y=2#frag",
		"http://sub.example.test:8443/form",
		"https://xn--bcher-kva.example",
		"https://example.test/path%20with%20encoding",
	} {
		if err := validURI(valid); err != nil {
			t.Errorf("validURI(%q) rejected a value the corpus relies on: %v", valid, err)
		}
	}
	if err := validateYAML(t, brokerWithWebForm("https://example.test/opt-out")); err != nil {
		t.Fatalf("a clean web_form channel must load: %v", err)
	}
}

// The cleaned corpus is the real evidence: all 1,279 files must load under the
// strict rule. This is the criterion #843 asked for.
func TestCleanedCorpusLoadsUnderTheStrictRule(t *testing.T) {
	brokers, err := LoadFromDir(filepath.Join(repoRoot(t), "registry"))
	if err != nil {
		t.Fatalf("the cleaned registry corpus must load: %v", err)
	}
	if len(brokers) < 1200 {
		t.Fatalf("expected the full corpus, loaded %d brokers", len(brokers))
	}
	// No value reaching a browser may be a non-URI any more.
	nonURI := 0
	for _, broker := range brokers {
		for _, channel := range broker.OptOut {
			if channel.Type == "web_form" && !strings.HasPrefix(channel.URL, "http") {
				nonURI++
			}
		}
	}
	if nonURI != 0 {
		t.Fatalf("%d web_form channels still carry a non-http url", nonURI)
	}
}
