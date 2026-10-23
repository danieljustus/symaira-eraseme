package registry

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"

	jsonschema "github.com/santhosh-tekuri/jsonschema/v6"
	"gopkg.in/yaml.v3"
)

// fixturesFS gives tests access to tests/fixtures/registry-contract via the
// repo layout (tests run with the package dir as CWD, so walk up).
func fixturesFS(t *testing.T) fs.FS {
	t.Helper()
	root := repoRoot(t)
	return os.DirFS(filepath.Join(root, "tests", "fixtures", "registry-contract"))
}

func repoRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("go.mod not found above package dir")
		}
		dir = parent
	}
}

// TestGoldenFixturesConformance: every golden fixture loads and validates;
// the invalid fixture is rejected (contract §7/§8).
func TestGoldenFixturesConformance(t *testing.T) {
	fix := fixturesFS(t)
	entries, err := fs.ReadDir(fix, ".")
	if err != nil {
		t.Fatal(err)
	}
	golden := 0
	for _, e := range entries {
		name := e.Name()
		content, err := fs.ReadFile(fix, name)
		if err != nil {
			t.Fatal(err)
		}
		d := &doc{id: strings.TrimSuffix(name, filepath.Ext(name)), path: name, content: content}
		b, err := decodeAndValidate(d)
		if strings.HasPrefix(name, "invalid-") {
			if err == nil {
				t.Errorf("%s: expected validation error, got none", name)
			} else if !errors.Is(err, &ValidationError{}) && !strings.Contains(err.Error(), "not in the closed enum") && !strings.Contains(err.Error(), "unknown top-level") {
				t.Errorf("%s: unexpected error type: %v", name, err)
			}
			continue
		}
		if err != nil {
			t.Errorf("%s: golden fixture must validate: %v", name, err)
			continue
		}
		golden++
		switch name {
		case "golden-webform-us.yaml":
			if len(b.OptOut) == 0 || b.OptOut[0].FormSpec == nil {
				t.Errorf("%s: expected web_form channel with form_spec", name)
			}
			if b.Status != "active" || b.DataSensitivity == nil {
				t.Errorf("%s: defaults not applied", name)
			}
		case "golden-email-eu.yaml":
			if len(b.Jurisdictions) < 2 {
				t.Errorf("%s: expected multi-jurisdiction", name)
			}
		case "golden-multi-uk.yaml":
			hasEmail, hasForm := false, false
			for _, c := range b.OptOut {
				if c.Type == "email" {
					hasEmail = true
				}
				if c.Type == "web_form" {
					hasForm = true
				}
			}
			if !hasEmail || !hasForm {
				t.Errorf("%s: expected both channel types", name)
			}
		case "golden-minimal-us.yaml":
			// disabled channel must still validate
			if b.OptOut[0].Disabled == nil || !*b.OptOut[0].Disabled {
				t.Errorf("%s: expected disabled channel", name)
			}
			if b.Verification != nil {
				t.Errorf("%s: verification must be nil when absent", name)
			}
		}
	}
	if golden < 4 {
		t.Errorf("expected at least 4 golden fixtures, validated %d", golden)
	}
}

// TestEmbeddedRegistryLoads: the embedded registry must exist (registered by
// regdata.go via an init() — in package-internal tests we register it from
// the repo root manually) and all 1,279 broker documents must load.
func TestEmbeddedRegistryLoads(t *testing.T) {
	root := repoRoot(t)
	// Register the real registry dir as the embedded FS (mirrors regdata.go).
	SetEmbedded(os.DirFS(filepath.Join(root, "registry")))
	brokers, err := LoadEmbedded()
	if err != nil {
		t.Fatalf("embedded registry failed to load: %v", err)
	}
	if got := len(brokers); got != 1277 {
		t.Errorf("expected 1,277 brokers from embedded registry (1,279 files minus 2 _example.yaml docs per contract §2), got %d", got)
	}
	// Spot-check sort order + defaults.
	if len(brokers) > 1 && brokers[0].ID >= brokers[len(brokers)-1].ID {
		t.Errorf("brokers not sorted by id")
	}
}

// TestLiveRegistryDirMatchesEmbedded: loading from the raw repo dir equals
// the registered embedded copy (same count, same ids).
func TestLiveRegistryDirMatchesEmbedded(t *testing.T) {
	root := repoRoot(t)
	viaDir, err := LoadFromDir(filepath.Join(root, "registry"))
	if err != nil {
		t.Fatalf("live dir failed: %v", err)
	}
	SetEmbedded(os.DirFS(filepath.Join(root, "registry")))
	viaEmbed, err := LoadEmbedded()
	if err != nil {
		t.Fatalf("embedded failed: %v", err)
	}
	if len(viaDir) != len(viaEmbed) {
		t.Fatalf("count mismatch: dir=%d embed=%d", len(viaDir), len(viaEmbed))
	}
	for i := range viaDir {
		if viaDir[i].ID != viaEmbed[i].ID {
			t.Fatalf("order mismatch at %d: %s vs %s", i, viaDir[i].ID, viaEmbed[i].ID)
		}
	}
}

// TestUnknownTopLevelFieldRejected: contract §3 additionalProperties:false.
func TestUnknownTopLevelFieldRejected(t *testing.T) {
	yamlSrc := []byte("id: test-broker\nname: Test\nwebsite: https://x.example.com\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@b.example.com\nbogus_field: 1\n")
	d := &doc{id: "test-broker", path: "brokers/us/test-broker.yaml", content: yamlSrc}
	if _, err := decodeAndValidate(d); err == nil {
		t.Fatal("expected rejection of unknown top-level field")
	}
}

// TestChannelVariantRules: email must not carry web_form fields and
// vice versa (contract §4 oneOf).
func TestChannelVariantRules(t *testing.T) {
	base := "id: t\nname: T\nwebsite: https://t.example.com\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n"
	cases := []struct {
		name    string
		channel string
		wantErr bool
	}{
		{"email with url", "  - type: email\n    endpoint: a@b.example.com\n    url: https://x.example.com\n", true},
		{"email with form_spec", "  - type: email\n    endpoint: a@b.example.com\n    form_spec:\n      steps:\n        - goto: https://x.example.com\n", true},
		{"web_form without form_spec", "  - type: web_form\n    url: https://x.example.com\n", true},
		{"web_form with endpoint", "  - type: web_form\n    url: https://x.example.com\n    form_spec:\n      steps:\n        - goto: https://x.example.com\n    endpoint: a@b.example.com\n", true},
		{"web_form valid", "  - type: web_form\n    url: https://x.example.com\n    form_spec:\n      steps:\n        - goto: https://x.example.com\n", false},
		{"email valid", "  - type: email\n    endpoint: a@b.example.com\n", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			d := &doc{id: "t", content: []byte(base + tc.channel)}
			_, err := decodeAndValidate(d)
			if tc.wantErr && err == nil {
				t.Fatalf("expected error, got none")
			}
			if !tc.wantErr && err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

// TestVerifySyncedSmoke: VerifySynced rejects a nonexistent dir.
func TestVerifySyncedSmoke(t *testing.T) {
	if _, err := VerifySynced(filepath.Join(t.TempDir(), "missing")); err == nil {
		t.Fatal("expected error for missing dir")
	}
}

func TestLoaderRejectsMultipleDocumentsBeforeSchemaDecode(t *testing.T) {
	base := []byte("id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n")
	for _, suffix := range []string{"---\nid: other\n", "...\nid: other\n", "---\nopt_out: [null, null]\n"} {
		source := append(append([]byte{}, base...), []byte(suffix)...)
		if _, err := decodeAndValidate(&doc{id: "test", content: source}); err == nil {
			t.Fatalf("expected multiple-document rejection for %q", suffix)
		}
	}
}

func TestLoaderRejectsNonfiniteFormTiming(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - goto: https://example.test\n"
	for _, field := range []string{"timeout_seconds", "rate_limit_delay"} {
		for _, value := range []string{".nan", ".inf", "-.inf"} {
			source := []byte(base + "      " + field + ": " + value + "\n")
			if _, err := decodeAndValidate(&doc{id: "test", content: source}); err == nil {
				t.Fatalf("expected nonfinite %s rejection for %s", field, value)
			}
		}
	}
}

func writeRegistryMetadata(t *testing.T, root, manifest, schema string) {
	t.Helper()
	if manifest != "" {
		if err := os.WriteFile(filepath.Join(root, "manifest.json"), []byte(manifest), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if schema != "" {
		if err := os.MkdirAll(filepath.Join(root, "schemas"), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "schemas", "broker.schema.json"), []byte(schema), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.MkdirAll(filepath.Join(root, "brokers", "us"), 0o755); err != nil {
		t.Fatal(err)
	}
}

func TestRegistryMetadataContract(t *testing.T) {
	validManifest := `{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}`
	validSchema := `{"schema_version":1}`
	cases := []struct{ name, manifest, schema, want string }{
		{"missing manifest", "", validSchema, "manifest.json"},
		{"malformed manifest", "{", validSchema, "malformed"},
		{"unknown manifest version", `{"schema_version":2,"schemas":{"broker":"schemas/broker.schema.json"}}`, validSchema, "schema_version"},
		{"noninteger manifest version", `{"schema_version":1.0,"schemas":{"broker":"schemas/broker.schema.json"}}`, validSchema, "schema_version"},
		{"malformed schema", validManifest, "{", "malformed"},
		{"mismatched schema", validManifest, `{"schema_version":2}`, "schema_version"},
		{"unsafe schema pointer", `{"schema_version":1,"schemas":{"broker":"../broker.schema.json"}}`, "", "schemas.broker"},
		{"missing schema", validManifest, "", "broker schema"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			root := t.TempDir()
			writeRegistryMetadata(t, root, tc.manifest, tc.schema)
			if _, err := LoadFromDir(root); err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error = %v, want substring %q", err, tc.want)
			}
		})
	}
	root := t.TempDir()
	writeRegistryMetadata(t, root, validManifest, validSchema)
	if brokers, err := LoadFromDir(root); err != nil || len(brokers) != 0 {
		t.Fatalf("valid metadata load = %v, %d brokers", err, len(brokers))
	}
	if brokers, err := Load(os.DirFS(root)); err != nil || len(brokers) != 0 {
		t.Fatalf("valid generic FS metadata load = %v, %d brokers", err, len(brokers))
	}
}

func TestSchemaChannelVariantsWithStandardsValidator(t *testing.T) {
	compiler := jsonschema.NewCompiler()
	schema, err := compiler.Compile(filepath.Join(repoRoot(t), "registry", "schemas", "broker.schema.json"))
	if err != nil {
		t.Fatalf("compile schema: %v", err)
	}
	base := map[string]any{"id": "schema-test", "name": "Schema Test", "website": "https://example.test", "category": "other", "jurisdictions": []any{"US"}, "laws": []any{"GDPR"}, "priority": "low"}
	withChannel := func(channel map[string]any) map[string]any {
		document := make(map[string]any, len(base)+1)
		for key, value := range base {
			document[key] = value
		}
		document["opt_out"] = []any{channel}
		return document
	}
	form := map[string]any{"steps": []any{map[string]any{"goto": "."}}}
	cases := []struct {
		name     string
		document map[string]any
		valid    bool
	}{
		{"valid email", withChannel(map[string]any{"type": "email", "endpoint": "a@example.test"}), true},
		{"valid web form", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": form}), true},
		{"email with url", withChannel(map[string]any{"type": "email", "endpoint": "a@example.test", "url": "https://example.test"}), false},
		{"email with form spec", withChannel(map[string]any{"type": "email", "endpoint": "a@example.test", "form_spec": form}), false},
		{"web form with endpoint", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": form, "endpoint": "a@example.test"}), false},
		{"valid universal selector", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"fill": map[string]any{"*": "x"}}}}}), true},
		{"valid tag selector", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"fill": map[string]any{"input": "x"}}}}}), true},
		{"invalid selector", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"fill": map[string]any{"@#": "x"}}}}}), false},
		{"empty goto", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"goto": ""}}}}), false},
		{"empty click", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"click": ""}}}}), false},
		{"empty wait for", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"wait_for": ""}}}}), false},
		{"empty screenshot", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"screenshot": ""}}}}), false},
		{"empty assert text", withChannel(map[string]any{"type": "web_form", "url": "https://example.test", "form_spec": map[string]any{"steps": []any{map[string]any{"assert_text": ""}}}}), false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := schema.Validate(tc.document)
			if tc.valid && err != nil {
				t.Fatalf("valid document rejected: %v", err)
			}
			if !tc.valid && err == nil {
				t.Fatal("mixed channel variant accepted")
			}
		})
	}
	if _, err := json.Marshal(base); err != nil {
		t.Fatal(err)
	}
}

func TestLiveRegistryMatchesJSONSchema(t *testing.T) {
	root := repoRoot(t)
	schema, err := jsonschema.NewCompiler().Compile(filepath.Join(root, "registry", "schemas", "broker.schema.json"))
	if err != nil {
		t.Fatalf("compile schema: %v", err)
	}
	brokersRoot := filepath.Join(root, "registry", "brokers")
	rootHandle, err := os.OpenRoot(brokersRoot)
	if err != nil {
		t.Fatalf("open broker root: %v", err)
	}
	defer rootHandle.Close()
	count, entries, aggregateBytes := 0, 0, 0
	err = filepath.WalkDir(brokersRoot, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		entries++
		if entries > maxDirectoryEntries {
			return fmt.Errorf("directory entry limit %d exceeded", maxDirectoryEntries)
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("symlink is not allowed: %s", path)
		}
		if entry.IsDir() || strings.HasPrefix(entry.Name(), "_") || (!strings.HasSuffix(entry.Name(), ".yaml") && !strings.HasSuffix(entry.Name(), ".yml")) {
			return nil
		}
		if count >= maxBrokerFiles {
			return fmt.Errorf("broker file limit %d exceeded", maxBrokerFiles)
		}
		relative, relErr := filepath.Rel(brokersRoot, path)
		if relErr != nil {
			return relErr
		}
		file, openErr := rootHandle.Open(relative)
		if openErr != nil {
			return openErr
		}
		info, statErr := file.Stat()
		if statErr != nil {
			file.Close()
			return statErr
		}
		if !info.Mode().IsRegular() {
			file.Close()
			return fmt.Errorf("schema fixture is not a regular file: %s", path)
		}
		if info.Size() > maxDocumentBytes {
			file.Close()
			return fmt.Errorf("schema fixture exceeds %d bytes: %s", maxDocumentBytes, path)
		}
		remaining := maxAggregateInputBytes - aggregateBytes
		if remaining <= 0 {
			file.Close()
			return fmt.Errorf("aggregate input byte limit %d exceeded", maxAggregateInputBytes)
		}
		limit := min(maxDocumentBytes, remaining)
		content, readErr := io.ReadAll(io.LimitReader(file, int64(limit)+1))
		closeErr := file.Close()
		if readErr != nil {
			return readErr
		}
		if closeErr != nil {
			return closeErr
		}
		if len(content) > limit {
			return fmt.Errorf("schema fixture byte limit exceeded: %s", path)
		}
		aggregateBytes += len(content)
		var document any
		if decodeErr := yaml.Unmarshal(content, &document); decodeErr != nil {
			return decodeErr
		}
		if validateErr := schema.Validate(document); validateErr != nil {
			return fmt.Errorf("%s: %w", path, validateErr)
		}
		count++
		return nil
	})
	if err != nil {
		t.Fatalf("validate live registry schema: %v", err)
	}
	if count != 1277 {
		t.Fatalf("validated %d broker documents, want 1277", count)
	}
}

func TestExplicitNullsRejectedAcrossRegistrySchema(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"
	web := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - goto: .\n"
	captcha := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - solve_captcha:\n            type: turnstile\n            site_key: 12345678\n"
	cases := []struct{ name, source string }{
		{"required id", "id: null\n"},
		{"data sensitivity", base + "data_sensitivity: null\n"},
		{"verification", base + "verification: null\n"},
		{"broker disabled", base + "disabled: null\n"},
		{"added date", base + "added_date: null\n"},
		{"source", base + "source: null\n"},
		{"status", base + "status: null\n"},
		{"notes", base + "notes: null\n"},
		{"channel endpoint", strings.Replace(base, "endpoint: a@example.test", "endpoint: null", 1)},
		{"channel template", base + "  - type: email\n    endpoint: b@example.test\n    template: null\n"},
		{"channel locale", base + "  - type: email\n    endpoint: b@example.test\n    locale: null\n"},
		{"channel required fields", base + "  - type: email\n    endpoint: b@example.test\n    required_fields: null\n"},
		{"channel suppression", base + "  - type: email\n    endpoint: b@example.test\n    supports_suppression: null\n"},
		{"channel response days", base + "  - type: email\n    endpoint: b@example.test\n    expected_response_days: null\n"},
		{"channel disabled", base + "  - type: email\n    endpoint: b@example.test\n    disabled: null\n"},
		{"web url", strings.Replace(web, "url: https://example.test", "url: null", 1)},
		{"form spec", strings.Replace(web, "form_spec:\n", "form_spec: null\n# ", 1)},
		{"verification ack", base + "verification:\n  ack_keywords: null\n"},
		{"form timeout", web + "      timeout_seconds: null\n"},
		{"form delay", web + "      rate_limit_delay: null\n"},
		{"form headless", web + "      headless: null\n"},
		{"step goto", strings.Replace(web, "goto: .", "goto: null", 1)},
		{"step fill", strings.Replace(web, "goto: .", "fill: null", 1)},
		{"step select", strings.Replace(web, "goto: .", "select: null", 1)},
		{"step click", strings.Replace(web, "goto: .", "click: null", 1)},
		{"step wait for", strings.Replace(web, "goto: .", "wait_for: null", 1)},
		{"step wait seconds", strings.Replace(web, "goto: .", "wait_seconds: null", 1)},
		{"step screenshot", strings.Replace(web, "goto: .", "screenshot: null", 1)},
		{"step assert text", strings.Replace(web, "goto: .", "assert_text: null", 1)},
		{"step captcha", strings.Replace(web, "goto: .", "solve_captcha: null", 1)},
		{"captcha provider", captcha + "            provider: null\n"},
		{"captcha action", captcha + "            action: null\n"},
		{"captcha min score", captcha + "            min_score: null\n"},
		{"captcha invisible", captcha + "            is_invisible: null\n"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := decodeAndValidate(&doc{id: "test", content: []byte(tc.source)}); err == nil {
				t.Fatal("explicit null accepted")
			}
		})
	}
}

func TestScalarPunctuationAndNodeContracts(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"
	for _, notes := range []string{"notes: hello &world\n", "notes: hello *world\n", "notes: hello#world\n", "notes: \"hello &world *world #world\"\n", "notes: |\n  hello &world *world #world\n  ---\n  ...\n", "notes: >\n  hello &world *world #world\n  ---\n  ...\n"} {
		if _, err := decodeAndValidate(&doc{id: "test", content: []byte(base + notes)}); err != nil {
			t.Errorf("scalar %q rejected: %v", notes, err)
		}
	}
	for _, marker := range []string{"&email a@example.test", "*email"} {
		source := []byte(strings.Replace(base, "endpoint: a@example.test", "endpoint: "+marker, 1))
		if _, err := decodeAndValidate(&doc{id: "test", content: source}); err == nil {
			t.Errorf("%s accepted", marker)
		}
	}
	quotedFlow := base + "verification:\n  ack_keywords: [ok,\"[\"]\n"
	if _, err := decodeAndValidate(&doc{id: "test", content: []byte(quotedFlow)}); err != nil {
		t.Errorf("quoted flow punctuation rejected: %v", err)
	}
}

func TestExplicitlyEmptyStringActionsAreRejectedWithOtherActions(t *testing.T) {
	base := "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - wait_seconds: 0\n"
	for _, field := range []string{"goto", "click", "wait_for", "screenshot", "assert_text"} {
		for _, value := range []string{"''", "!custom ''", `!custom ""`} {
			source := base + "          " + field + ": " + value + "\n"
			if _, err := decodeAndValidate(&doc{id: "test", content: []byte(source)}); err == nil {
				t.Errorf("explicitly empty %s value %s accepted alongside another action", field, value)
			}
		}
	}
	validNestedEmpty := strings.Replace(base, "        - wait_seconds: 0\n", "        - fill:\n            input: ''\n", 1)
	if _, err := decodeAndValidate(&doc{id: "test", content: []byte(validNestedEmpty)}); err != nil {
		t.Errorf("empty nested fill value rejected: %v", err)
	}
}

func TestLoadFromDirEnforcesCanonicalBrokerLayout(t *testing.T) {
	manifest := `{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}`
	schema := `{"schema_version":1}`
	base := []byte("id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n")
	for _, relative := range []string{"brokers/direct.yaml", "brokers/ca/test.yaml", "brokers/us/deep/test.yaml"} {
		root := t.TempDir()
		writeRegistryMetadata(t, root, manifest, schema)
		path := filepath.Join(root, relative)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, base, 0o644); err != nil {
			t.Fatal(err)
		}
		if _, err := LoadFromDir(root); err == nil || !strings.Contains(err.Error(), "layout") {
			t.Fatalf("%s: error=%v", relative, err)
		}
	}

	root := t.TempDir()
	writeRegistryMetadata(t, root, manifest, schema)
	doc := filepath.Join(root, "brokers", "us", "_example.yaml")
	if err := os.WriteFile(doc, base, 0o644); err != nil {
		t.Fatal(err)
	}
	if brokers, err := LoadFromDir(root); err != nil || len(brokers) != 0 {
		t.Fatalf("underscore documentation file: brokers=%d err=%v", len(brokers), err)
	}
}
