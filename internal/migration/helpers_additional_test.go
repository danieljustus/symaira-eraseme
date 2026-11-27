package migration

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func TestMigrationHelperBranches(t *testing.T) {
	source := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "file"), []byte("content"), 0o640); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(source, "dir"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(source, "file"), filepath.Join(source, "link")); err != nil {
		t.Fatal(err)
	}
	if err := copyTree(source, filepath.Join(t.TempDir(), "copy")); err == nil {
		t.Fatal("copyTree accepted a symlink")
	}
	if err := copyOne(filepath.Join(source, "dir"), filepath.Join(t.TempDir(), "dir")); err == nil {
		t.Fatal("copyOne accepted a directory")
	}
	if err := copyOne(filepath.Join(source, "missing"), filepath.Join(t.TempDir(), "missing")); err == nil {
		t.Fatal("copyOne accepted a missing source")
	}
	copy := filepath.Join(t.TempDir(), "file")
	if err := copyOne(filepath.Join(source, "file"), copy); err != nil {
		t.Fatal(err)
	}
	if string(mustReadFile(t, copy)) != "content" {
		t.Fatal("copyOne did not preserve content")
	}
	statePath := filepath.Join(t.TempDir(), "nested", "state.json")
	if err := writeState(statePath, state{Version: stateVersion, SourceRoot: source, DestinationRoot: "destination", BackupDir: "backup"}); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(statePath); err != nil {
		t.Fatal(err)
	}
	if err := writeAtomic(filepath.Join(t.TempDir(), "nested", "data"), []byte("atomic"), 0o600); err != nil {
		t.Fatal(err)
	}

	var detection Detection
	addScheduler(&detection, "missing.service", "", "/tmp/missing.service", false, "reason")
	if len(detection.Artifacts) != 1 || detection.Artifacts[0].Source != "" || detection.Artifacts[0].Kind != KindScheduler {
		t.Fatalf("scheduler artifact = %#v", detection.Artifacts)
	}
	for _, platform := range []scheduler.Platform{scheduler.PlatformLaunchd, scheduler.PlatformSystemd, scheduler.PlatformCron, "unknown"} {
		root, names := nativeSchedulerPaths("/tmp/home", platform)
		if platform == scheduler.PlatformLaunchd && (root == "" || len(names) == 0) {
			t.Fatal("launchd paths missing")
		}
		if platform == scheduler.PlatformSystemd && (root == "" || len(names) == 0) {
			t.Fatal("systemd paths missing")
		}
		if platform == scheduler.PlatformCron && root != "" || platform == "unknown" && root != "" {
			t.Fatalf("unexpected native paths for %s: %q", platform, root)
		}
	}
	if !isNativeSchedulerFile("job.plist", scheduler.PlatformLaunchd) || isNativeSchedulerFile("job.service", scheduler.PlatformLaunchd) || !isNativeSchedulerFile("job.timer", scheduler.PlatformSystemd) || isNativeSchedulerFile("job.plist", scheduler.PlatformSystemd) {
		t.Fatal("native scheduler file classification mismatch")
	}
	generated, err := generatedSchedulerFiles(Options{BinaryPath: "/usr/local/bin/symeraseme", ProjectDir: "/tmp/project", SchedulerConfig: scheduler.Config{OutputDir: filepath.Join(t.TempDir(), "schedules")}}, scheduler.PlatformCron, t.TempDir())
	if err != nil || len(generated) == 0 {
		t.Fatalf("generated scheduler files = %d, err=%v", len(generated), err)
	}
}
