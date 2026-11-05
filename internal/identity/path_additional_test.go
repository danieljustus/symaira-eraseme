package identity

import (
	"path/filepath"
	"testing"
)

func TestWithinDirAndExpiryHelpers(t *testing.T) {
	dir := t.TempDir()
	if !withinDir(dir, filepath.Join(dir, "child", "file")) {
		t.Fatal("path inside directory rejected")
	}
	if withinDir(dir, filepath.Join(dir, "..", "outside")) {
		t.Fatal("path outside directory accepted")
	}
	if value, err := ParseExpiry("1234"); err != nil || value != 1234 {
		t.Fatalf("valid expiry = %d, err=%v", value, err)
	}
}
