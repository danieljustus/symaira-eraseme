package migration

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func TestMigrationPathAndStateValidation(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	if _, _, err := validateRoots("", destination); err == nil {
		t.Fatal("empty source accepted")
	}
	if _, _, err := validateRoots(source, source); err == nil {
		t.Fatal("identical roots accepted")
	}
	if _, _, err := validateRoots(source, filepath.Join(source, "nested")); err == nil {
		t.Fatal("nested destination accepted")
	}
	resolvedSource, resolvedDestination, err := validateRoots(source, destination)
	if err != nil || resolvedSource != source || resolvedDestination != destination {
		t.Fatalf("valid roots = %q, %q, err=%v", resolvedSource, resolvedDestination, err)
	}
	if err := os.MkdirAll(destination, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := absoluteDir(""); err == nil {
		t.Fatal("empty absolute path accepted")
	}

	configSource := t.TempDir()
	configDestination := t.TempDir()
	cs, cd, err := configRoots(Options{SourceConfigRoot: configSource, DestinationConfigRoot: configDestination}, source, destination)
	if err != nil || cs != configSource || cd != configDestination {
		t.Fatalf("config roots = %q, %q, err=%v", cs, cd, err)
	}
	if _, _, err := configRoots(Options{SourceConfigRoot: destination}, source, destination); err == nil {
		t.Fatal("overlapping config root accepted")
	}

	for label := range map[string]struct{}{
		"home": {}, "scheduler source": {}, "scheduler destination": {},
		"binary": {}, "project directory": {}, "scheduler output": {},
	} {
		opts := Options{}
		switch label {
		case "home":
			opts.HomeDir = "relative"
		case "scheduler source":
			opts.SchedulerSource = "relative"
		case "scheduler destination":
			opts.SchedulerDest = "relative"
		case "binary":
			opts.BinaryPath = "relative"
		case "project directory":
			opts.ProjectDir = "relative"
		case "scheduler output":
			opts.SchedulerConfig.OutputDir = "relative"
		}
		if err := validateAuxiliaryPaths(opts); err == nil {
			t.Errorf("relative %s path accepted", label)
		}
	}
	if err := validateAuxiliaryPaths(Options{HomeDir: t.TempDir(), Platform: scheduler.PlatformLaunchd}); err != nil {
		t.Fatal(err)
	}
	if err := validateMigrationPaths(Options{}, source, destination, source, destination); err != nil {
		t.Fatal(err)
	}
	if err := validateMigrationPaths(Options{}, source, destination, filepath.Join(destination, "config"), destination); err == nil {
		t.Fatal("overlapping source config accepted")
	}
	file := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(file, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := validateOptionalDirectoryPath("file", file); err == nil {
		t.Fatal("regular file accepted as directory")
	}
	if err := validateOptionalDirectoryPath("missing", filepath.Join(t.TempDir(), "missing")); err != nil {
		t.Fatal(err)
	}
	if err := validateBackupDir(filepath.Join(t.TempDir(), "backup"), source, destination); err != nil {
		t.Fatal(err)
	}
	if err := validateBackupDir(source, source); err == nil {
		t.Fatal("overlapping backup accepted")
	}
	if !pathsOverlap(source, source) || !pathsOverlap(source, filepath.Join(source, "child")) || pathsOverlap(source, destination) {
		t.Fatal("path overlap semantics mismatch")
	}
	if !pathWithinAny(filepath.Join(source, "child"), "", source) || pathWithinAny(destination, source) {
		t.Fatal("pathWithinAny semantics mismatch")
	}

	statePath := filepath.Join(destination, "state.json")
	if _, found, err := loadState(statePath, source, destination); err != nil || found {
		t.Fatalf("missing state = found %v, err=%v", found, err)
	}
	if err := os.WriteFile(statePath, []byte("{"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := loadState(statePath, source, destination); err == nil || !strings.Contains(err.Error(), "decode") {
		t.Fatal("malformed state accepted")
	}
	for _, st := range []state{
		{Version: 2, SourceRoot: source, DestinationRoot: destination, BackupDir: "/tmp/backup"},
		{Version: stateVersion, SourceRoot: "/other", DestinationRoot: destination, BackupDir: "/tmp/backup"},
		{Version: stateVersion, SourceRoot: source, DestinationRoot: destination},
	} {
		data, marshalErr := json.Marshal(st)
		if marshalErr != nil {
			t.Fatal(marshalErr)
		}
		if err := os.WriteFile(statePath, data, 0o600); err != nil {
			t.Fatal(err)
		}
		if _, _, err := loadState(statePath, source, destination); err == nil {
			t.Fatal("invalid state accepted")
		}
	}
	valid := state{Version: stateVersion, SourceRoot: source, DestinationRoot: destination, BackupDir: filepath.Join(t.TempDir(), "backup"), Items: map[string]string{"event-db": "done"}}
	data, err := json.Marshal(valid)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(statePath, data, 0o600); err != nil {
		t.Fatal(err)
	}
	loaded, found, err := loadState(statePath, source, destination)
	if err != nil || !found || loaded.BackupDir != valid.BackupDir || loaded.Items["event-db"] != "done" {
		t.Fatalf("valid state = %#v, found=%v, err=%v", loaded, found, err)
	}
}
