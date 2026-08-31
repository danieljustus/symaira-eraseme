package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/spf13/cobra"
)

func childCommand(parentName string, root *cobra.Command) *cobra.Command {
	for _, command := range root.Commands() {
		if command.Name() == parentName {
			return command
		}
	}
	return nil
}

func TestCommandVocabularyMatchesGoContracts(t *testing.T) {
	root := newRootCommand()
	for _, name := range []string{
		"version", "init-profile", "show-profile", "render-template", "grant",
		"plan", "tick", "status", "calendar", "dashboard", "generate-report",
		"generate-dashboard", "generate-scheduler", "schedule", "registry",
		"requests", "events", "brokers", "manual-tasks", "review", "config", "mcp",
	} {
		if childCommand(name, root) == nil {
			t.Errorf("missing top-level command %q", name)
		}
	}
	plan := childCommand("plan", root)
	for _, name := range []string{"create", "show", "execute", "tick", "status"} {
		if childCommand(name, plan) == nil {
			t.Errorf("missing plan command %q", name)
		}
	}
	schedule := childCommand("schedule", root)
	for _, name := range []string{"install", "uninstall", "status"} {
		if childCommand(name, schedule) == nil {
			t.Errorf("missing schedule command %q", name)
		}
	}
}

func TestTickJSONSmokeUsesStableEnvelope(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	out, err := execute(t, "tick", "--dry-run", "--output", "json")
	if err != nil {
		t.Fatalf("tick --dry-run failed: %v\n%s", err, out)
	}
	var payload map[string]any
	if err := json.Unmarshal([]byte(out), &payload); err != nil {
		t.Fatalf("tick output is not JSON: %v\n%s", err, out)
	}
	if payload["success"] != true {
		t.Fatalf("success = %#v, want true", payload["success"])
	}
	if _, ok := payload["actions"]; !ok {
		t.Fatalf("actions missing from payload: %s", out)
	}
	if _, err := os.Stat(filepath.Join(dataDir, "symeraseme.db")); err != nil {
		t.Fatalf("tick did not initialize database: %v", err)
	}
}

func TestConfigJSONSmokeDoesNotLeakSecrets(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	out, err := execute(t, "config", "show", "--output", "json")
	if err != nil {
		t.Fatalf("config show failed: %v\n%s", err, out)
	}
	if !strings.Contains(out, `"success":true`) || strings.Contains(out, "PASSWORD") {
		t.Fatalf("unexpected config payload: %s", out)
	}
}
