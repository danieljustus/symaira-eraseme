package migration

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestArtifactDataBackupAndSafeNames(t *testing.T) {
	source := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "config.toml"), []byte("value"), 0o640); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(source, "nested"), 0o700); err != nil {
		t.Fatal(err)
	}
	if _, mode, err := artifactData(Artifact{ID: "config", Source: filepath.Join(source, "config.toml")}, nil); err != nil || mode.Perm() != 0o640 {
		t.Fatalf("regular artifact = mode %v, err=%v", mode, err)
	}
	if data, mode, err := artifactData(Artifact{ID: "script", Kind: KindScheduler, Destination: "/tmp/tick.sh"}, map[string]string{"tick.sh": "#!/bin/sh\n"}); err != nil || string(data) != "#!/bin/sh\n" || mode.Perm() != 0o755 {
		t.Fatalf("scheduler artifact = %q, mode %v, err=%v", data, mode, err)
	}
	for _, artifact := range []Artifact{
		{ID: "missing-source"},
		{ID: "missing-file", Source: filepath.Join(source, "missing")},
		{ID: "directory", Source: filepath.Join(source, "nested")},
		{ID: "missing-scheduler", Kind: KindScheduler, Destination: "/tmp/missing.sh"},
	} {
		if _, _, err := artifactData(artifact, nil); err == nil {
			t.Fatalf("invalid artifact accepted: %#v", artifact)
		}
	}

	extra := t.TempDir()
	if err := os.WriteFile(filepath.Join(extra, "config.toml"), []byte("extra"), 0o600); err != nil {
		t.Fatal(err)
	}
	external := filepath.Join(t.TempDir(), "../external.txt")
	if err := os.WriteFile(external, []byte("external"), 0o600); err != nil {
		t.Fatal(err)
	}
	backup := filepath.Join(t.TempDir(), "backup")
	artifacts := []Artifact{
		{ID: "config", Source: filepath.Join(source, "config.toml")},
		{ID: "external", Source: external},
	}
	if err := ensureBackup(backup, source, []string{extra}, artifacts); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(backup, ".complete.json")); err != nil {
		t.Fatal(err)
	}
	if err := ensureBackup(backup, source, []string{extra}, artifacts); err != nil {
		t.Fatalf("valid backup resume failed: %v", err)
	}
	stale := filepath.Join(t.TempDir(), "stale")
	if err := os.Mkdir(stale, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := ensureBackup(stale, source, nil, nil); err == nil {
		t.Fatal("incomplete backup accepted")
	}
	invalid := filepath.Join(t.TempDir(), "invalid")
	if err := os.Mkdir(invalid, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(invalid, ".complete.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ensureBackup(invalid, source, nil, nil); err == nil {
		t.Fatal("invalid backup marker accepted")
	}
	if safeName("../secret") != "__secret" || safeName("a/b") != "a_b" || safeName("") != "artifact" {
		t.Fatal("safeName contract mismatch")
	}
	if !strings.Contains(string(mustReadFile(t, filepath.Join(backup, ".complete.json"))), "external") {
		t.Fatal("external backup missing from manifest")
	}
}

func mustReadFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}
