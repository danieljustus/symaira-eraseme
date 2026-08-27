package main

import (
	"bytes"
	"encoding/json"
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

func TestVersionCommandUnknownSubcommandFails(t *testing.T) {
	_, err := execute(t, "definitely-not-a-command")
	if err == nil {
		t.Fatal("unknown subcommand should fail")
	}
}
