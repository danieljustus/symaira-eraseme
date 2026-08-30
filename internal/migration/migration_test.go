package migration

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func Test728DetectsPythonEraArtifactsWithoutReadingSecretValues(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "go-state")
	home := t.TempDir()
	writeFixture(t, filepath.Join(source, "symeraseme.db"), "synthetic sqlite bytes")
	writeFixture(t, filepath.Join(source, "config.toml"), "[app]\nmode = 'fixture'\n")
	writeFixture(t, filepath.Join(source, "identity.enc"), "synthetic encrypted profile")
	writeFixture(t, filepath.Join(source, "schedules", "symeraseme-tick.sh"), "#!/bin/sh\n/usr/bin/python3 -m symeraseme tick\n")
	writeFixture(t, filepath.Join(home, ".config", "systemd", "user", "symeraseme-tick.service"), "ExecStart=/usr/bin/python3 -m symeraseme tick\n")

	detection, err := Detect(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, HomeDir: home,
		Platform: scheduler.PlatformSystemd, BinaryPath: "/opt/symeraseme",
	})
	if err != nil {
		t.Fatal(err)
	}
	if !detection.Detected || !strings.Contains(detection.Summary, "Python-era") {
		t.Fatalf("detection = %+v", detection)
	}
	kinds := map[ArtifactKind]bool{}
	for _, item := range detection.Artifacts {
		kinds[item.Kind] = true
	}
	for _, kind := range []ArtifactKind{KindEventDB, KindConfig, KindProfile, KindSecrets, KindScheduler} {
		if !kinds[kind] {
			t.Errorf("detected artifact kind %q; artifacts = %+v", kind, detection.Artifacts)
		}
	}
	if detection.SecretStore.Migratable {
		t.Fatal("metadata-only secret store unexpectedly claims it can migrate")
	}
}

func Test728DryRunMakesNoFilesystemMutation(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	backup := filepath.Join(t.TempDir(), "backup")
	writeFixture(t, filepath.Join(source, "symeraseme.db"), "synthetic db")
	writeFixture(t, filepath.Join(source, "identity.enc"), "synthetic profile")

	report, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, BackupDir: backup,
		Platform: scheduler.PlatformCron, DryRun: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !report.DryRun || len(report.Items) == 0 {
		t.Fatalf("dry-run report = %+v", report)
	}
	if exists(destination) || exists(backup) {
		t.Fatalf("dry-run created destination=%v or backup=%v", exists(destination), exists(backup))
	}
	if got := string(mustRead(t, filepath.Join(source, "symeraseme.db"))); got != "synthetic db" {
		t.Fatalf("source changed during dry-run: %q", got)
	}
}

func Test728BackupPrecedesWritesAndResumeSkipsCompletedItems(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	backup := filepath.Join(t.TempDir(), "backup")
	writeFixture(t, filepath.Join(source, "symeraseme.db"), "original db")
	writeFixture(t, filepath.Join(source, "config.toml"), "original config")

	failed := false
	first, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, BackupDir: backup,
		Platform: scheduler.PlatformCron,
		BeforeItem: func(a Artifact) error {
			if a.ID == "event-db" && !failed {
				failed = true
				return errors.New("synthetic interruption")
			}
			return nil
		},
	})
	if err == nil || first == nil {
		t.Fatalf("interrupted migration err=%v report=%+v", err, first)
	}
	if got := string(mustRead(t, filepath.Join(destination, "config.toml"))); got != "original config" {
		t.Fatalf("completed item was not written: %q", got)
	}
	if got := string(mustRead(t, filepath.Join(filepath.Join(backup, "source"), "symeraseme.db"))); got != "original db" {
		t.Fatalf("backup was not made before writes: %q", got)
	}
	if got := string(mustRead(t, filepath.Join(source, "symeraseme.db"))); got != "original db" {
		t.Fatalf("source was modified: %q", got)
	}

	second, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, BackupDir: backup,
		Platform: scheduler.PlatformCron,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !second.Resumed || !second.Complete {
		t.Fatalf("resume report = %+v", second)
	}
	foundSkipped := false
	for _, item := range second.Items {
		if item.Artifact.ID == "config:config.toml" && item.Status == "skipped" {
			foundSkipped = true
		}
	}
	if !foundSkipped {
		t.Fatalf("resume did not skip completed config item: %+v", second.Items)
	}
	if _, err := os.Stat(filepath.Join(destination, ".migration-state.json")); err != nil {
		t.Fatalf("migration state missing: %v", err)
	}
}

type fakeSecretStore struct {
	inspectCalls int
	migrateCalls int
	report       SecretReport
}

func (f *fakeSecretStore) Inspect(context.Context, string) (SecretReport, error) {
	f.inspectCalls++
	return f.report, nil
}

func (f *fakeSecretStore) Migrate(context.Context, string) error {
	f.migrateCalls++
	return nil
}

func Test728MigratesNativeSchedulerUnitsToGeneratedGoContent(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	backup := filepath.Join(t.TempDir(), "backup")
	home := t.TempDir()
	writeFixture(t, filepath.Join(source, "symeraseme.db"), "synthetic db")
	native := filepath.Join(home, ".config", "systemd", "user")
	legacy := filepath.Join(native, "symeraseme-tick.service")
	writeFixture(t, legacy, "ExecStart=/usr/bin/python3 -m symeraseme tick\n")

	report, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, BackupDir: backup, HomeDir: home,
		Platform: scheduler.PlatformSystemd, BinaryPath: "/opt/symeraseme", ProjectDir: "/srv/erase",
	})
	if err != nil {
		t.Fatal(err)
	}
	if !report.Complete {
		t.Fatalf("native scheduler migration incomplete: %+v", report)
	}
	content := string(mustRead(t, legacy))
	if strings.Contains(content, "python3") || strings.Contains(content, "__WRAPPER_DIR__") {
		t.Fatalf("native unit was not converted to Go content: %s", content)
	}
	if got := string(mustRead(t, filepath.Join(backup, "external", "000-symeraseme-tick.service"))); got != "ExecStart=/usr/bin/python3 -m symeraseme tick\n" {
		t.Fatalf("native unit backup missing original content: %q", got)
	}
}

func Test728SecretMigrationIsExplicitAndInjectable(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(t.TempDir(), "destination")
	backup := filepath.Join(t.TempDir(), "backup")
	writeFixture(t, filepath.Join(source, "identity.enc"), "synthetic encrypted profile")
	store := &fakeSecretStore{report: SecretReport{Detected: true, ReferenceCount: 2, Description: "synthetic keyring references", Migratable: true}}
	manualReport, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: filepath.Join(t.TempDir(), "manual"),
		BackupDir: filepath.Join(t.TempDir(), "manual-backup"), Platform: scheduler.PlatformCron,
		SecretStore: store,
	})
	if err != nil || manualReport.Complete || store.migrateCalls != 0 {
		t.Fatalf("default secret handling copied or completed: report=%+v err=%v calls=%d", manualReport, err, store.migrateCalls)
	}

	report, err := Run(context.Background(), Options{
		SourceRoot: source, DestinationRoot: destination, BackupDir: backup,
		Platform: scheduler.PlatformCron, SecretStore: store, CopySecrets: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	if store.inspectCalls != 2 || store.migrateCalls != 1 || !report.Complete {
		t.Fatalf("secret migration = report=%+v inspect=%d migrate=%d", report, store.inspectCalls, store.migrateCalls)
	}
	for _, item := range report.Items {
		if strings.Contains(item.Artifact.Source, "REAL_SECRET") || strings.Contains(item.Artifact.Destination, "REAL_SECRET") {
			t.Fatal("secret value-like data appeared in migration metadata")
		}
	}
}

func Test728MigratesSeparatePythonConfigDirectory(t *testing.T) {
	dataSource := t.TempDir()
	configSource := t.TempDir()
	destination := filepath.Join(t.TempDir(), "data")
	configDestination := filepath.Join(t.TempDir(), "config")
	backup := filepath.Join(t.TempDir(), "backup")
	writeFixture(t, filepath.Join(dataSource, "symeraseme.db"), "synthetic db")
	writeFixture(t, filepath.Join(configSource, "config.toml"), "[app]\nfixture = true\n")
	writeFixture(t, filepath.Join(configSource, "identity.enc"), "synthetic profile")
	store := &fakeSecretStore{report: SecretReport{Detected: true, Description: "synthetic references", Migratable: true}}

	report, err := Run(context.Background(), Options{
		SourceRoot: dataSource, DestinationRoot: destination,
		SourceConfigRoot: configSource, DestinationConfigRoot: configDestination,
		BackupDir: backup, Platform: scheduler.PlatformCron, SecretStore: store, CopySecrets: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !report.Complete {
		t.Fatalf("separate config migration incomplete: %+v", report)
	}
	if got := string(mustRead(t, filepath.Join(configDestination, "config.toml"))); got != "[app]\nfixture = true\n" {
		t.Fatalf("config was not migrated: %q", got)
	}
	if got := string(mustRead(t, filepath.Join(configDestination, "identity.encrypted"))); got != "synthetic profile" {
		t.Fatalf("profile was not migrated: %q", got)
	}
	if _, err := os.Stat(filepath.Join(backup, "config-0", "identity.enc")); err != nil {
		t.Fatalf("separate config source was not backed up: %v", err)
	}
}

func Test728RejectsNestedDestinationBeforeMutation(t *testing.T) {
	source := t.TempDir()
	destination := filepath.Join(source, "destination")
	writeFixture(t, filepath.Join(source, "symeraseme.db"), "synthetic db")
	if _, err := Run(context.Background(), Options{SourceRoot: source, DestinationRoot: destination, DryRun: true}); err == nil {
		t.Fatal("nested destination was accepted")
	}
	if exists(filepath.Join(source, ".migration-state.json")) {
		t.Fatal("invalid migration created state")
	}
}

func writeFixture(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
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
