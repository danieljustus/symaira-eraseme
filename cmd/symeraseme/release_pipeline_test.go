package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

type workflowDoc struct {
	Name string                 `yaml:"name"`
	Jobs map[string]workflowJob `yaml:"jobs"`
}

type workflowJob struct {
	Name        string         `yaml:"name"`
	RunsOn      string         `yaml:"runs-on"`
	Environment string         `yaml:"environment"`
	Needs       any            `yaml:"needs"`
	Steps       []workflowStep `yaml:"steps"`
}

type workflowStep struct {
	Name string            `yaml:"name"`
	Uses string            `yaml:"uses"`
	Run  string            `yaml:"run"`
	If   string            `yaml:"if"`
	Env  map[string]string `yaml:"env"`
}

func loadReleaseWorkflow(t *testing.T) (string, workflowDoc) {
	t.Helper()
	candidates := []string{
		filepath.Join("..", "..", ".github", "workflows", "release.yml"),
		filepath.Join(".github", "workflows", "release.yml"),
	}
	var data []byte
	var loadedPath string
	var err error
	for _, p := range candidates {
		data, err = os.ReadFile(p)
		if err == nil {
			loadedPath = p
			break
		}
	}
	if err != nil || len(data) == 0 {
		t.Fatalf("failed to read .github/workflows/release.yml: %v", err)
	}

	var doc workflowDoc
	if err := yaml.Unmarshal(data, &doc); err != nil {
		t.Fatalf("failed to parse YAML in %s: %v", loadedPath, err)
	}
	return string(data), doc
}

func getWorkflowStep(t *testing.T, steps []workflowStep, name string) workflowStep {
	t.Helper()
	for _, s := range steps {
		if s.Name == name {
			return s
		}
	}
	t.Fatalf("step %q not found in release-gui job", name)
	return workflowStep{}
}

func TestReleaseWorkflowContract(t *testing.T) {
	rawYAML, doc := loadReleaseWorkflow(t)

	guiJob, ok := doc.Jobs["release-gui"]
	if !ok {
		t.Fatalf("missing job 'release-gui' in release workflow")
	}

	t.Run("TopLevelAndJobHierarchy", func(t *testing.T) {
		if doc.Name != "Release" {
			t.Errorf("workflow name: got %q, want %q", doc.Name, "Release")
		}
		if _, ok := doc.Jobs["release-cli"]; !ok {
			t.Errorf("missing job 'release-cli' in release workflow")
		}
		if guiJob.RunsOn != "macos-15" {
			t.Errorf("release-gui runs-on: got %q, want %q", guiJob.RunsOn, "macos-15")
		}
		if guiJob.Environment != "release" {
			t.Errorf("release-gui environment: got %q, want %q", guiJob.Environment, "release")
		}
		switch v := guiJob.Needs.(type) {
		case string:
			if v != "release-cli" {
				t.Errorf("release-gui needs: got %q, want %q", v, "release-cli")
			}
		case []any:
			if len(v) != 1 || v[0] != "release-cli" {
				t.Errorf("release-gui needs: got %v, want ['release-cli']", v)
			}
		default:
			t.Errorf("unexpected type for release-gui needs: %T (%v)", guiJob.Needs, guiJob.Needs)
		}
	})

	t.Run("ExactStepOrder", func(t *testing.T) {
		var stepNames []string
		for _, s := range guiJob.Steps {
			if s.Name != "" {
				stepNames = append(stepNames, s.Name)
			}
		}

		expectedOrder := []string{
			"Checkout workflow source",
			"Set up Go",
			"Import Developer ID certificate",
			"Validate notarization credentials",
			"Build and sign macOS app bundle",
			"Notarize and staple macOS app",
			"Create and sign DMG container",
			"Notarize DMG",
			"Staple and Gatekeeper-verify DMG",
			"Upload GUI DMG to release",
			"Verify published DMG asset and record release",
			"Clean up signing material",
		}
		if !reflect.DeepEqual(stepNames, expectedOrder) {
			t.Fatalf("unexpected step order in release-gui job:\ngot:  %v\nwant: %v", stepNames, expectedOrder)
		}
	})

	t.Run("ImportDeveloperIDCertificate", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Import Developer ID certificate")
		for _, envVar := range []string{"CERTIFICATE_P12", "CERTIFICATE_PASSWORD", "KEYCHAIN_PASSWORD"} {
			if _, ok := step.Env[envVar]; !ok {
				t.Errorf("missing expected env var %q in step %q", envVar, step.Name)
			}
		}
		for _, expected := range []string{
			`trap 'rm -f "$CERTIFICATE_PATH"' EXIT`,
			"security create-keychain",
			"security import",
			"Developer ID Application",
			`echo "CODESIGN_IDENTITY=$IDENTITY" >> "$GITHUB_ENV"`,
			`echo "KEYCHAIN_PATH=$KEYCHAIN_PATH" >> "$GITHUB_ENV"`,
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected run command %q", step.Name, expected)
			}
		}
	})

	t.Run("ValidateNotarizationCredentials", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Validate notarization credentials")
		for _, envVar := range []string{"NOTARY_API_KEY", "NOTARY_API_KEY_ID", "NOTARY_API_ISSUER"} {
			if _, ok := step.Env[envVar]; !ok {
				t.Errorf("missing expected env var %q in step %q", envVar, step.Name)
			}
		}
		for _, expected := range []string{
			"for name in NOTARY_API_KEY NOTARY_API_KEY_ID NOTARY_API_ISSUER; do",
			`echo "::error::$name secret is required for GUI release notarization."`,
			"exit 1",
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing fail-closed check %q", step.Name, expected)
			}
		}
	})

	t.Run("BuildAndSignAppBundle", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Build and sign macOS app bundle")
		for _, expected := range []string{
			"REQUIRE_SIGNING=true",
			"./scripts/package-dmg.sh --app-only",
			`APP_PATH="app/SymairaEraseMe/.build/dmg-stage/Symaira EraseMe.app"`,
			`test -d "$APP_PATH"`,
			`test -x "$APP_PATH/Contents/MacOS/Symaira EraseMe"`,
			`test -x "$APP_PATH/Contents/MacOS/symeraseme"`,
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}
	})

	t.Run("NotarizeAndStapleApp", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Notarize and staple macOS app")
		for _, expected := range []string{
			"ditto -c -k --sequesterRsrc --keepParent",
			"xcrun notarytool submit",
			"--output-format json",
			"| tee",
			`test "$(jq -r '.status' "$RESPONSE_PATH")" = "Accepted"`,
			"xcrun stapler staple",
			"xcrun stapler validate",
			"codesign --verify --deep --strict",
			"spctl --assess --type execute",
			"trap cleanup EXIT",
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}
	})

	t.Run("CreateAndSignDMGContainer", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Create and sign DMG container")
		for _, expected := range []string{
			"REQUIRE_SIGNING=true",
			"./scripts/package-dmg.sh --dmg-only",
			`echo "DMG_PATH=$DMG" >> "$GITHUB_ENV"`,
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}
	})

	t.Run("NotarizeDMG", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Notarize DMG")
		for _, expected := range []string{
			"xcrun notarytool submit",
			"--output-format json",
			"| tee",
			`test "$(jq -r '.status' "$RESPONSE_PATH")" = "Accepted"`,
			"trap cleanup EXIT",
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}
	})

	t.Run("StapleAndGatekeeperVerifyDMG", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Staple and Gatekeeper-verify DMG")
		for _, expected := range []string{
			`xcrun stapler staple "$DMG_PATH"`,
			`xcrun stapler validate "$DMG_PATH"`,
			"codesign --verify --strict",
			"spctl --assess --type open --context context:primary-signature",
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}
	})

	t.Run("VerifyPublishedDMGAssetAndRecordRelease", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Verify published DMG asset and record release")
		for _, expected := range []string{
			"gh release download",
			`cmp -s "$DMG_PATH" "$PUBLISHED_PATH"`,
			"codesign --verify --strict",
			"xcrun stapler validate",
			"spctl --assess --type open --context context:primary-signature",
			"checksums.txt",
			"gh release upload",
			"gh release edit",
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected assertion %q", step.Name, expected)
			}
		}

		// Enforce no || true or touch fallback in published verification
		if strings.Contains(step.Run, "|| true") {
			t.Errorf("step %q contains forbidden '|| true' fallback/suppression", step.Name)
		}
		if strings.Contains(step.Run, "touch") {
			t.Errorf("step %q contains forbidden 'touch' empty-file fallback", step.Name)
		}

		// Enforce full order inside published verification:
		// download DMG -> exact SHA/cmp -> codesign -> stapler validate -> spctl -> download existing checksums -> mutate/upload checksums -> edit notes
		type orderedStage struct {
			name    string
			pattern string
		}
		stages := []orderedStage{
			{name: "download DMG", pattern: `--pattern "$ASSET_NAME"`},
			{name: "exact SHA/cmp", pattern: `cmp -s "$DMG_PATH" "$PUBLISHED_PATH"`},
			{name: "codesign", pattern: `codesign --verify --strict --verbose=2 "$PUBLISHED_PATH"`},
			{name: "stapler validate", pattern: `xcrun stapler validate "$PUBLISHED_PATH"`},
			{name: "spctl", pattern: `spctl --assess --type open --context context:primary-signature`},
			{name: "download existing checksums", pattern: `--pattern checksums.txt`},
			{name: "mutate checksums", pattern: `python3 - "$CHECKSUMS"`},
			{name: "upload checksums", pattern: `gh release upload "$RELEASE_TAG" "$CHECKSUMS"`},
			{name: "edit notes", pattern: `gh release edit "$RELEASE_TAG"`},
		}

		lastIdx := -1
		lastName := ""
		for _, stage := range stages {
			idx := strings.Index(step.Run, stage.pattern)
			if idx == -1 {
				t.Fatalf("step %q missing pattern for stage %q: %q", step.Name, stage.name, stage.pattern)
			}
			if idx <= lastIdx {
				t.Errorf("step %q stage order violation: %q (idx %d) must appear after %q (idx %d)",
					step.Name, stage.name, idx, lastName, lastIdx)
			}
			lastIdx = idx
			lastName = stage.name
		}
	})

	t.Run("CleanUpSigningMaterial", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Clean up signing material")
		if step.If != "always()" {
			t.Errorf("step %q if-condition: got %q, want %q", step.Name, step.If, "always()")
		}
		for _, expected := range []string{
			"KEYCHAIN_PATH:-$RUNNER_TEMP/symaira-release.keychain-db",
			"security delete-keychain",
			`rm -f "$RUNNER_TEMP"/*.p12`,
			`"$RUNNER_TEMP"/*.p8`,
			`"$RUNNER_TEMP"/*.zip`,
			`"$RUNNER_TEMP"/*notary*.json`,
		} {
			if !strings.Contains(step.Run, expected) {
				t.Errorf("step %q missing expected cleanup command %q", step.Name, expected)
			}
		}
	})

	t.Run("NoUnsignedFallback", func(t *testing.T) {
		if strings.Contains(rawYAML, "ad-hoc/unsigned") {
			t.Errorf("workflow contains forbidden fallback string 'ad-hoc/unsigned'")
		}
		if strings.Contains(rawYAML, "The GUI DMG is ad-hoc") {
			t.Errorf("workflow contains forbidden fallback string 'The GUI DMG is ad-hoc'")
		}
	})

	t.Run("NoChecksumManifestFallback", func(t *testing.T) {
		step := getWorkflowStep(t, guiJob.Steps, "Verify published DMG asset and record release")
		if strings.Contains(step.Run, "checksums.txt") && strings.Contains(step.Run, "|| true") {
			t.Errorf("step %q contains forbidden '|| true' fallback for checksums download", step.Name)
		}
		if strings.Contains(step.Run, "touch") {
			t.Errorf("step %q contains forbidden 'touch' fallback", step.Name)
		}
	})
}

func TestPackageDMGMockSuite(t *testing.T) {
	scriptPath := filepath.Join("..", "..", "tests", "test_release_dmg.sh")
	cmd := exec.Command("bash", scriptPath)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("test_release_dmg.sh failed: %v\nOutput:\n%s", err, string(out))
	}
}
