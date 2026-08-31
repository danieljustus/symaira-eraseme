package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCLIReadOnlyAndDryRunSurfaces(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	cases := [][]string{
		{"--output", "json", "status"},
		{"--output", "json", "plan", "show"},
		{"--output", "json", "plan", "status"},
		{"--output", "json", "tick", "--dry-run"},
		{"--output", "json", "calendar"},
		{"--output", "json", "dashboard"},
		{"--output", "json", "requests", "list"},
		{"--output", "json", "events", "show", "999"},
		{"--output", "json", "manual-tasks", "list"},
		{"--output", "json", "manual-tasks", "show", "999"},
		{"--output", "json", "manual-tasks", "complete", "999"},
		{"--output", "json", "manual-tasks", "cleanup", "--dry-run"},
		{"--output", "json", "generate-scheduler", "--platform", "cron", "--dry-run"},
		{"--output", "json", "schedule", "install", "--platform", "cron", "--dry-run"},
		{"--output", "json", "schedule", "status", "--platform", "cron"},
	}
	for _, args := range cases {
		name := strings.Join(args, " ")
		t.Run(name, func(t *testing.T) {
			out, err := execute(t, args...)
			if err != nil {
				t.Fatalf("command failed: %v\n%s", err, out)
			}
			if !json.Valid([]byte(out)) {
				t.Fatalf("command did not return valid JSON: %s", out)
			}
		})
	}
}

func TestCLIGenerateReportText(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	out, err := execute(t, "generate-report", "--all-campaigns")
	if err != nil || !strings.Contains(out, "success") {
		t.Fatalf("generate-report failed: %v\\n%s", err, out)
	}
}

func TestCLIProfileTemplateAndAdapterPaths(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", strings.Repeat("a", 64))

	profileOut, err := execute(t, "--output", "json", "init-profile", "--full-name", "Test Person", "--email", "test@example.test")
	if err != nil || !strings.Contains(profileOut, "profile_path") {
		t.Fatalf("init-profile failed: %v\\n%s", err, profileOut)
	}
	showOut, err := execute(t, "--output", "json", "show-profile")
	if err != nil || !strings.Contains(showOut, "Test Person") || strings.Contains(showOut, "master_key") {
		t.Fatalf("show-profile failed or leaked data: %v\\n%s", err, showOut)
	}
	templateOut, err := execute(t, "render-template", "gdpr-art17.en.md.j2", "--broker-name", "Example")
	if err != nil || templateOut == "" {
		t.Fatalf("render-template failed: %v\\n%s", err, templateOut)
	}
	grantOut, err := execute(t, "--output", "json", "grant", "execute", "--dry-run")
	if err != nil || !strings.Contains(grantOut, "dry_run") {
		t.Fatalf("grant dry-run failed: %v\\n%s", err, grantOut)
	}
	webOut, err := execute(t, "--output", "json", "run-web-form", "missing", "--dry-run")
	if err != nil || !strings.Contains(webOut, "success") {
		t.Fatalf("web-form dry-run failed: %v\\n%s", err, webOut)
	}
	autoOut, err := execute(t, "--output", "json", "auto-confirm", "999", "--dry-run")
	if err != nil || !strings.Contains(autoOut, "no inbox reply") {
		t.Fatalf("auto-confirm dry-run failed: %v\\n%s", err, autoOut)
	}
	reportPath := filepath.Join(t.TempDir(), "dashboard.html")
	reportOut, err := execute(t, "generate-dashboard", "--output", reportPath)
	if err != nil || !strings.Contains(reportOut, "success") {
		t.Fatalf("generate-dashboard failed: %v\\n%s", err, reportOut)
	}
	if _, err := os.Stat(reportPath); err != nil {
		t.Fatalf("dashboard output missing: %v", err)
	}
	if _, err := execute(t, "poll-inbox", "--host", "imap.example.test"); err == nil {
		t.Fatal("poll-inbox unexpectedly succeeded without an IMAP dialer")
	}
}

func TestCLIEmbeddedRegistrySurfaces(t *testing.T) {
	for _, args := range [][]string{
		{"--output", "json", "registry", "list"},
		{"--output", "json", "registry", "validate"},
		{"--output", "json", "brokers", "list", "--jurisdiction", "DE"},
	} {
		out, err := execute(t, args...)
		if err != nil {
			t.Fatalf("%v failed: %v\n%s", args, err, out)
		}
		if !strings.Contains(out, "schema_version") {
			t.Fatalf("%v returned unexpected output: %s", args, out)
		}
	}
}

func TestCLICompletionSupportsAllShellsAndRejectsUnknown(t *testing.T) {
	for _, shell := range []string{"bash", "zsh", "fish", "powershell"} {
		out, err := execute(t, "completion", shell)
		if err != nil || !strings.Contains(out, "symeraseme") {
			t.Fatalf("completion %s failed: %v\n%s", shell, err, out)
		}
	}
	if _, err := execute(t, "completion", "unknown"); err == nil {
		t.Fatal("unknown completion shell accepted")
	}
}

func TestCLIArgumentAndOutputValidation(t *testing.T) {
	if value, err := intArgument(nil, 4, "request ID"); err != nil || value != 4 {
		t.Fatalf("fallback int argument = %d, err=%v", value, err)
	}
	if _, err := intArgument([]string{"bad"}, 4, "request ID"); err == nil {
		t.Fatal("invalid positional integer accepted")
	}
	if _, err := execute(t, "--output", "yaml", "status"); err == nil {
		t.Fatal("invalid output format accepted")
	}
	if _, err := execute(t, "review"); err == nil {
		t.Fatal("review without a file path accepted")
	}
}
