package scheduler

import (
	"context"
	"encoding/xml"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestGenerateIsDeterministicAndUsesExactBinaryPath(t *testing.T) {
	cfg := Config{
		Platform:   PlatformCron,
		ProjectDir: "/work/project with spaces",
		BinaryPath: "/opt/symaira/bin/symeraseme",
		TickHour:   6,
		TickMinute: 45,
		PollHours:  []int{8, 20},
	}
	first, err := Generate(cfg)
	if err != nil {
		t.Fatal(err)
	}
	second, err := Generate(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(first, second) {
		t.Fatal("same config produced different scheduler files")
	}
	tick := first["symeraseme-tick.sh"]
	if !strings.Contains(tick, "exec '/opt/symaira/bin/symeraseme' 'tick' '--output' 'json'") {
		t.Fatalf("tick wrapper does not preserve exact binary path:\n%s", tick)
	}
	if !strings.Contains(tick, "cd '/work/project with spaces'") {
		t.Fatalf("project path is not shell quoted:\n%s", tick)
	}
	if strings.Contains(tick, "exec symeraseme ") {
		t.Fatal("wrapper fell back to a PATH-dependent binary name")
	}
	if !strings.Contains(first["crontab.txt"], "45 6 * * * {wrapper_dir}/symeraseme-tick.sh") {
		t.Fatal("custom tick time missing from crontab")
	}
	if !strings.Contains(first["symeraseme-poll.sh"], "08:00|20:00") {
		t.Fatal("poll hours are not deterministic or ordered")
	}
}

func TestGenerateAllBackends(t *testing.T) {
	cfg := Config{BinaryPath: "/usr/local/bin/symeraseme", ProjectDir: "/srv/erase", TickHour: 10, PollHours: []int{8, 12, 16, 20}}

	cron, err := Generate(Config{Platform: PlatformCron, BinaryPath: cfg.BinaryPath, ProjectDir: cfg.ProjectDir, TickHour: cfg.TickHour, PollHours: cfg.PollHours})
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"symeraseme-tick.sh", "symeraseme-poll.sh", "symeraseme-rescan.sh", "crontab.txt", "install.sh", "uninstall.sh"} {
		if cron[name] == "" {
			t.Errorf("cron file %q missing", name)
		}
	}
	if !strings.Contains(cron["symeraseme-rescan.sh"], "'--dry-run'") {
		t.Error("cron rescan does not preserve Python dry-run behavior")
	}

	launchd, err := Generate(Config{Platform: PlatformLaunchd, BinaryPath: cfg.BinaryPath, ProjectDir: cfg.ProjectDir, TickHour: cfg.TickHour, PollHours: cfg.PollHours})
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"com.symeraseme.tick.plist", "com.symeraseme.poll.plist", "com.symeraseme.rescan.plist"} {
		if err := xml.Unmarshal([]byte(launchd[name]), new(any)); err != nil {
			t.Errorf("launchd file %q is not XML: %v", name, err)
		}
	}
	if got := strings.Count(launchd["com.symeraseme.poll.plist"], "<key>Hour</key>"); got != 4 {
		t.Errorf("poll launchd intervals = %d, want 4", got)
	}
	if !strings.Contains(launchd["com.symeraseme.rescan.plist"], "<integer>10</integer>") {
		t.Error("quarterly launchd schedule missing October interval")
	}

	systemd, err := Generate(Config{Platform: PlatformSystemd, BinaryPath: cfg.BinaryPath, ProjectDir: cfg.ProjectDir, TickHour: cfg.TickHour, PollHours: cfg.PollHours})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(systemd["symeraseme-tick.timer"], "OnCalendar=Daily-10:00") {
		t.Error("systemd daily timer is incorrect")
	}
	if !strings.Contains(systemd["symeraseme-poll.timer"], "OnCalendar=Hourly") || !strings.Contains(systemd["symeraseme-rescan.timer"], "OnCalendar=Quarterly") {
		t.Error("systemd timer cadence is incorrect")
	}
	if !strings.Contains(systemd["symeraseme-tick.service"], `ExecStart=/bin/bash "__WRAPPER_DIR__/symeraseme-tick.sh"`) {
		t.Error("systemd wrapper placeholder is missing")
	}
}

func TestGenerateValidationAndDefaultBinary(t *testing.T) {
	if _, err := Generate(Config{Platform: Platform("windows")}); err == nil {
		t.Fatal("unsupported platform accepted")
	}
	if _, err := Generate(Config{Platform: PlatformCron, TickHour: 24}); err == nil {
		t.Fatal("invalid tick hour accepted")
	}
	if _, err := Generate(Config{Platform: PlatformCron, PollHours: []int{25}}); err == nil {
		t.Fatal("invalid poll hour accepted")
	}
	path, err := ResolveBinaryPath("")
	if err != nil {
		t.Fatal(err)
	}
	if !filepath.IsAbs(path) {
		t.Fatalf("default binary path is not absolute: %q", path)
	}
}

func TestWriteFilesSortsAndSetsWrapperModes(t *testing.T) {
	out := filepath.Join(t.TempDir(), "nested", "schedules")
	written, err := WriteFiles(out, map[string]string{"z.txt": "z", "a.sh": "#!/bin/sh\n"})
	if err != nil {
		t.Fatal(err)
	}
	if got := filepath.Base(written[0]); got != "a.sh" {
		t.Fatalf("files are not written deterministically: %v", written)
	}
	if mode := mustStat(t, written[1]).Mode().Perm(); mode != 0o644 {
		t.Fatalf("text mode = %o, want 644", mode)
	}
	if mode := mustStat(t, written[0]).Mode().Perm(); mode != 0o755 {
		t.Fatalf("wrapper mode = %o, want 755", mode)
	}
	if _, err := WriteFiles(out, map[string]string{"../escape": "no"}); err == nil {
		t.Fatal("path traversal accepted")
	}
}

func mustStat(t *testing.T, path string) os.FileInfo {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	return info
}

func TestLegacyPythonDetection(t *testing.T) {
	legacy := filepath.Join(t.TempDir(), "symeraseme-tick.service")
	if err := os.WriteFile(legacy, []byte("ExecStart=/bin/bash /home/me/.venv/bin/python\n# Generated by symeraseme generate-scheduler\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	got, err := DetectLegacyPythonUnit(legacy)
	if err != nil {
		t.Fatal(err)
	}
	if !got {
		t.Fatal("Python scheduler unit was not detected")
	}
	goWrapper := filepath.Join(t.TempDir(), "symeraseme-tick.sh")
	if err := os.WriteFile(goWrapper, []byte("#!/usr/bin/env bash\n# Generated by symeraseme generate-scheduler (Go)\nexec '/tmp/symeraseme' tick\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	got, err = DetectLegacyPythonUnit(goWrapper)
	if err != nil {
		t.Fatal(err)
	}
	if got {
		t.Fatal("Go-generated wrapper was misclassified as Python")
	}
	units, err := DetectLegacyPythonUnits([]string{legacy, filepath.Join(filepath.Dir(legacy), "missing")})
	if err != nil {
		t.Fatal(err)
	}
	if len(units) != 1 || !units[0].IsPython {
		t.Fatalf("unexpected detected units: %+v", units)
	}
}

func TestInstallRefusesLegacyUntilExplicitReplacement(t *testing.T) {
	home := t.TempDir()
	native := filepath.Join(home, ".config", "systemd", "user")
	if err := os.MkdirAll(native, 0o700); err != nil {
		t.Fatal(err)
	}
	legacyPath := filepath.Join(native, "symeraseme-tick.service")
	legacyContent := "ExecStart=/usr/bin/python3 -m symeraseme tick\n"
	if err := os.WriteFile(legacyPath, []byte(legacyContent), 0o644); err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "schedules")
	result, err := Install(context.Background(), InstallOptions{
		Config:  Config{Platform: PlatformSystemd, OutputDir: out, BinaryPath: "/opt/symaira/symeraseme", ProjectDir: "/srv/erase"},
		HomeDir: home,
		Runner:  &recordingRunner{},
	})
	if !errors.Is(err, ErrLegacyUnits) {
		t.Fatalf("install error = %v, want ErrLegacyUnits", err)
	}
	if !result.ReplacementRequired || len(result.Legacy) != 1 || !result.Legacy[0].IsPython {
		t.Fatalf("legacy replacement offer missing: %+v", result)
	}
	if got := string(mustRead(t, legacyPath)); got != legacyContent {
		t.Fatal("legacy unit was modified without replacement consent")
	}
}

func TestInstallReplacementWritesNativeUnitsAndExactPath(t *testing.T) {
	home := t.TempDir()
	native := filepath.Join(home, ".config", "systemd", "user")
	if err := os.MkdirAll(native, 0o700); err != nil {
		t.Fatal(err)
	}
	legacyPath := filepath.Join(native, "symeraseme-tick.service")
	if err := os.WriteFile(legacyPath, []byte("ExecStart=/usr/bin/python3 -m symeraseme tick\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	runner := &recordingRunner{}
	out := filepath.Join(t.TempDir(), "schedules")
	result, err := Install(context.Background(), InstallOptions{
		Config:  Config{Platform: PlatformSystemd, OutputDir: out, BinaryPath: "/opt/symaira/bin/symeraseme", ProjectDir: "/srv/erase"},
		HomeDir: home, ReplaceLegacy: true, Runner: runner,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.ReplacementRequired || len(result.Files) != 11 {
		t.Fatalf("unexpected install result: %+v", result)
	}
	service := string(mustRead(t, filepath.Join(native, "symeraseme-tick.service")))
	if !strings.Contains(service, `ExecStart=/bin/bash "`+out+`/symeraseme-tick.sh"`) {
		t.Fatalf("installed service did not resolve wrapper path: %s", service)
	}
	wrapper := string(mustRead(t, filepath.Join(out, "symeraseme-tick.sh")))
	if !strings.Contains(wrapper, "'/opt/symaira/bin/symeraseme'") {
		t.Fatal("installed wrapper lost exact binary path")
	}
	if got := runner.commands[0]; got != "systemctl --user daemon-reload" {
		t.Fatalf("first native command = %q", got)
	}
	if strings.Contains(service, "python3") {
		t.Fatal("legacy Python content survived replacement")
	}
}

func TestStatusAndUninstallSystemdUserUnits(t *testing.T) {
	home := t.TempDir()
	native := filepath.Join(home, ".config", "systemd", "user")
	if err := os.MkdirAll(native, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"symeraseme-tick", "symeraseme-poll", "symeraseme-rescan"} {
		for _, ext := range []string{".service", ".timer"} {
			if err := os.WriteFile(filepath.Join(native, name+ext), []byte("[Unit]\n"), 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
	runner := &recordingRunner{}
	status, err := Status(context.Background(), InstallOptions{Config: Config{Platform: PlatformSystemd}, HomeDir: home, Runner: runner})
	if err != nil {
		t.Fatal(err)
	}
	if len(status.Entries) != 3 || !status.Entries[0].Installed || status.Entries[0].Active {
		t.Fatalf("unexpected systemd status: %+v", status)
	}
	if err := Uninstall(context.Background(), InstallOptions{Config: Config{Platform: PlatformSystemd}, HomeDir: home, Runner: runner}); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"symeraseme-tick", "symeraseme-poll", "symeraseme-rescan"} {
		for _, ext := range []string{".service", ".timer"} {
			if _, err := os.Stat(filepath.Join(native, name+ext)); !os.IsNotExist(err) {
				t.Errorf("unit %s%s remains after uninstall", name, ext)
			}
		}
	}
}

func TestRemoveCronBlockPreservesUnrelatedEntries(t *testing.T) {
	input := "MAILTO=ops@example.test\n# Symaira EraseMe scheduled tasks\n0 10 * * * old-python\n# End Symaira EraseMe scheduled tasks\n15 3 * * * backup\n"
	got := removeCronBlock(input)
	if got != "MAILTO=ops@example.test\n15 3 * * * backup\n" {
		t.Fatalf("cron block removal changed unrelated entries: %q", got)
	}
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

type recordingRunner struct{ commands []string }

func (r *recordingRunner) Run(_ context.Context, name string, args ...string) ([]byte, error) {
	r.commands = append(r.commands, strings.Join(append([]string{name}, args...), " "))
	if name == "systemctl" && len(args) >= 3 && args[len(args)-2] == "is-active" {
		return []byte("inactive\n"), errors.New("inactive")
	}
	return nil, nil
}
