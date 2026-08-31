package main

import (
	"bytes"
	"context"
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

func TestMCPCommandExposesHTTPFlagsAndDefaultsToHTTP(t *testing.T) {
	cmd := newMCPCommand()
	for _, name := range []string{"host", "port", "allow-remote", "stdio"} {
		if cmd.Flag(name) == nil {
			t.Fatalf("mcp is missing --%s", name)
		}
	}
	if cmd.Flag("host").DefValue != "127.0.0.1" {
		t.Fatalf("mcp host default = %q, want 127.0.0.1", cmd.Flag("host").DefValue)
	}
	if cmd.Flag("port").DefValue != "8000" {
		t.Fatalf("mcp port default = %q, want 8000", cmd.Flag("port").DefValue)
	}
	if cmd.Flag("stdio").DefValue != "false" {
		t.Fatal("mcp must use HTTP by default")
	}
}

func TestServeAliasIsHiddenAndKeepsHTTPFlags(t *testing.T) {
	cmd := newServeAlias()
	if !cmd.Hidden {
		t.Fatal("serve compatibility alias should be hidden")
	}
	if cmd.Flag("host") == nil || cmd.Flag("port") == nil {
		t.Fatal("serve compatibility alias must expose HTTP flags")
	}
}

func TestLoopbackHostValidation(t *testing.T) {
	for _, host := range []string{"localhost", "127.0.0.1", "127.42.0.9", "::1", "[::1]"} {
		if !isLoopbackHost(host) {
			t.Errorf("isLoopbackHost(%q) = false, want true", host)
		}
	}
	for _, host := range []string{"0.0.0.0", "192.168.1.2", "example.invalid"} {
		if isLoopbackHost(host) {
			t.Errorf("isLoopbackHost(%q) = true, want false", host)
		}
	}
}

func TestWriteMCPAuthTokenUsesPrivateDataDirectory(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)

	token, path, err := writeMCPAuthToken()
	if err != nil {
		t.Fatalf("writeMCPAuthToken returned error: %v", err)
	}
	if len(token) < 40 {
		t.Fatalf("generated token is unexpectedly short: %d characters", len(token))
	}
	if path != filepath.Join(dataDir, "mcp_token") {
		t.Fatalf("token path = %q, want under configured data directory", path)
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read token file: %v", err)
	}
	if string(contents) != token {
		t.Fatal("token file contents do not match returned token")
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat token file: %v", err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("token file mode = %o, want 600", info.Mode().Perm())
	}
}

func TestMCPHTTPRejectsUnsafeConfiguration(t *testing.T) {
	if err := runMCPHTTP(context.Background(), "0.0.0.0", 8000, false); err == nil {
		t.Fatal("non-loopback bind accepted without --allow-remote")
	}
	if err := runMCPHTTP(context.Background(), "127.0.0.1", 0, false); err == nil {
		t.Fatal("port zero accepted")
	}
}
