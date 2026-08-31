package manualtasks

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInstructionsAndScreenshotEvidence(t *testing.T) {
	for _, reason := range FallbackReasons {
		text := InstructionsForReason(reason, "Example Broker", "")
		if text == "" || !strings.Contains(text, "Example Broker") {
			t.Errorf("instructions for %s = %q", reason, text)
		}
	}
	if text := InstructionsForReason("unknown", "Example Broker", ""); text == "" || !strings.Contains(text, "manually") {
		t.Fatalf("unknown reason instructions = %q", text)
	}
	if path, err := SaveScreenshot(nil); err != nil || path != "" {
		t.Fatalf("empty screenshot = %q, err=%v", path, err)
	}
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	path, err := SaveScreenshot([]byte("PNG test evidence"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(path, filepath.Join(dataDir, "manual_tasks")) {
		t.Fatalf("screenshot path = %q", path)
	}
	contents, err := os.ReadFile(path)
	if err != nil || string(contents) != "PNG test evidence" {
		t.Fatalf("screenshot contents = %q, err=%v", contents, err)
	}
	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("screenshot mode = %v, err=%v", info.Mode().Perm(), err)
	}
}
