package scheduler

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestSchedulerDefaultsAndPlatformDetection(t *testing.T) {
	cfg := DefaultConfig()
	if cfg.TickHour != 10 || cfg.OutputDir != "./schedules" || len(cfg.PollHours) != 4 {
		t.Fatalf("DefaultConfig = %#v", cfg)
	}
	if runtime.GOOS == "darwin" && DetectPlatform() != PlatformLaunchd {
		t.Fatalf("DetectPlatform() = %q on macOS", DetectPlatform())
	}
	files, err := GenerateSchedulerConfigs(Config{Platform: PlatformCron, BinaryPath: "/bin/symeraseme", ProjectDir: "/tmp/project"})
	if err != nil || len(files) == 0 {
		t.Fatalf("GenerateSchedulerConfigs = %d files, err=%v", len(files), err)
	}
	if _, err := WriteSchedulerFiles(t.TempDir(), files); err != nil {
		t.Fatal(err)
	}
}

func TestScanLegacyUnitsAcrossPlatforms(t *testing.T) {
	home := t.TempDir()
	launchd := filepath.Join(home, "Library", "LaunchAgents")
	if err := os.MkdirAll(launchd, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(launchd, "com.symeraseme.tick.plist"), []byte("python3 -m symeraseme"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(launchd, "com.symeraseme.poll.plist"), []byte("native symeraseme"), 0o600); err != nil {
		t.Fatal(err)
	}
	units, err := ScanLegacyPythonUnits(home, PlatformLaunchd)
	if err != nil || len(units) != 2 || !units[0].IsPython || units[1].IsPython {
		t.Fatalf("launchd units = %#v, err=%v", units, err)
	}
	if units[0].Kind != "launchd" || units[0].Platform != PlatformLaunchd {
		t.Fatalf("launchd metadata = %#v", units[0])
	}
	if units, err := ScanLegacyPythonUnits(home, PlatformCron); err != nil || units != nil {
		t.Fatalf("cron legacy scan = %#v, err=%v", units, err)
	}
}

func TestCronInstallStatusAndUninstallUseRunner(t *testing.T) {
	out := t.TempDir()
	runner := &recordingRunner{commands: nil}
	result, err := Install(context.Background(), InstallOptions{
		Config: Config{Platform: PlatformCron, OutputDir: out, BinaryPath: "/opt/symeraseme"},
		Runner: runner,
	})
	if err != nil || len(result.Files) == 0 {
		t.Fatalf("cron install = %#v, err=%v", result, err)
	}
	if len(runner.commands) < 2 || runner.commands[0] != "crontab -l" {
		t.Fatalf("cron runner commands = %#v", runner.commands)
	}
	status, err := Status(context.Background(), InstallOptions{Config: Config{Platform: PlatformCron}, Runner: runner})
	if err != nil || len(status.Entries) != 1 || status.Entries[0].Label != "cron" {
		t.Fatalf("cron status = %#v, err=%v", status, err)
	}
	if err := Uninstall(context.Background(), InstallOptions{Config: Config{Platform: PlatformCron}, Runner: runner}); err != nil {
		t.Fatal(err)
	}
}

func TestSchedulerFailureBranches(t *testing.T) {
	if _, err := Status(context.Background(), InstallOptions{Config: Config{Platform: "invalid"}}); err == nil {
		t.Fatal("invalid status platform accepted")
	}
	if err := Uninstall(context.Background(), InstallOptions{Config: Config{Platform: "invalid"}}); err == nil {
		t.Fatal("invalid uninstall platform accepted")
	}
	if _, err := WriteFiles("", map[string]string{}); err == nil {
		t.Fatal("empty output directory accepted")
	}
	if _, err := WriteFiles(t.TempDir(), map[string]string{filepath.Join("..", "escape"): "bad"}); err == nil {
		t.Fatal("traversal output name accepted")
	}
	if _, err := (ExecRunner{}).Run(context.Background(), "/bin/echo", "ok"); err != nil {
		t.Fatal(err)
	}
}
