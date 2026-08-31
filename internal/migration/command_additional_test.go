package migration

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func TestMigrationCommandOutputAndPlatformHelpers(t *testing.T) {
	command := NewCommand()
	if command == nil || command.Use != "migrate" {
		t.Fatalf("migration command = %#v", command)
	}
	if schedulerPlatform("launchd") != scheduler.PlatformLaunchd || schedulerPlatform("systemd") != scheduler.PlatformSystemd || schedulerPlatform("cron") != scheduler.PlatformCron {
		t.Fatal("scheduler platform mapping mismatch")
	}
	report := &Report{Detection: Detection{Summary: "migration summary"}, DryRun: true, Complete: true}
	var jsonOutput bytes.Buffer
	if err := writeJSON(&jsonOutput, report); err != nil || !json.Valid(jsonOutput.Bytes()) {
		t.Fatalf("JSON output = %q, err=%v", jsonOutput.String(), err)
	}
	var textOutput bytes.Buffer
	writeText(&textOutput, report)
	if !strings.Contains(textOutput.String(), "migration summary") || !strings.Contains(textOutput.String(), "Dry run") {
		t.Fatalf("text output = %q", textOutput.String())
	}
}
