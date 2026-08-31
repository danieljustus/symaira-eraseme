package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/versionkit"
)

func execute(t *testing.T, args ...string) (string, error) {
	t.Helper()
	root := newRootCommand()
	var buf bytes.Buffer
	root.SetOut(&buf)
	root.SetErr(&buf)
	root.SetArgs(args)
	err := root.Execute()
	return buf.String(), err
}

func TestVersionFlagPrintsAndExitsZero(t *testing.T) {
	out, err := execute(t, "--version")
	if err != nil {
		t.Fatalf("--version returned error: %v", err)
	}
	if out == "" {
		t.Fatal("--version printed nothing")
	}
}

func TestVersionCommandJSONIsHandshakePayload(t *testing.T) {
	out, err := execute(t, "version", "--json")
	if err != nil {
		t.Fatalf("version --json returned error: %v", err)
	}
	var info versionkit.Info
	if err := json.Unmarshal([]byte(out), &info); err != nil {
		t.Fatalf("version --json output is not valid JSON: %v\n%s", err, out)
	}
	if info.Tool != "symeraseme" {
		t.Errorf("tool = %q, want symeraseme", info.Tool)
	}
	if info.SchemaVersion != 1 {
		t.Errorf("schema_version = %d, want 1", info.SchemaVersion)
	}
}

func TestVersionCommandPlainText(t *testing.T) {
	out, err := execute(t, "version")
	if err != nil {
		t.Fatalf("version returned error: %v", err)
	}
	// versionkit Info.String() renders "tool vX.Y.Z" — with dev fallback:
	// "symeraseme dev" (versionkit appends "v" only when missing).
	if len(out) == 0 {
		t.Fatal("version printed nothing")
	}
}

func TestMigrateCommandDryRunIsRegistered(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "symeraseme.db"), []byte("synthetic db"), 0o600); err != nil {
		t.Fatal(err)
	}
	out, err := execute(t, "migrate", "--source", source, "--destination", destination, "--home", home, "--platform", "cron", "--dry-run", "--json")
	if err != nil {
		t.Fatalf("migrate dry-run returned error: %v", err)
	}
	if !strings.Contains(out, `"dry_run": true`) || !strings.Contains(out, "Python-era event database found") {
		t.Fatalf("unexpected migrate output: %s", out)
	}
	if _, err := os.Stat(destination); !os.IsNotExist(err) {
		t.Fatalf("dry-run created destination: %v", err)
	}
}

func TestVersionCommandUnknownSubcommandFails(t *testing.T) {
	_, err := execute(t, "definitely-not-a-command")
	if err == nil {
		t.Fatal("unknown subcommand should fail")
	}
}
