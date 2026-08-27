package registry

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
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
