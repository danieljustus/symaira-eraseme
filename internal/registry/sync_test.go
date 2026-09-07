package registry

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSyncRejectsArchiveTraversalAndPreservesOldState(t *testing.T) {
	dst := t.TempDir()
	old := filepath.Join(dst, "sentinel")
	if err := os.WriteFile(old, []byte("old-bytes"), 0o600); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(syncTestArchive(t))
	}))
	t.Cleanup(server.Close)

	if err := Sync(context.Background(), dst, server.URL); err == nil {
		t.Fatal("unsafe archive unexpectedly installed")
	}
	got, err := os.ReadFile(old)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old-bytes" {
		t.Fatalf("old state changed to %q", got)
	}
}

func TestSyncInstallsValidatedArchiveAtomically(t *testing.T) {
	dst := t.TempDir()
	old := filepath.Join(dst, "sentinel")
	if err := os.WriteFile(old, []byte("old-bytes"), 0o600); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(syncValidArchive(t))
	}))
	t.Cleanup(server.Close)

	if err := Sync(context.Background(), dst, server.URL); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(old); !os.IsNotExist(err) {
		t.Fatalf("old state remains after replacement: %v", err)
	}
	count, err := VerifySynced(dst)
	if err != nil || count != 1 {
		t.Fatalf("installed registry count=%d err=%v", count, err)
	}
	mode := fileMode(t, filepath.Join(dst, "manifest.json"))
	if mode.Perm() != 0o600 {
		t.Fatalf("manifest mode = %o", mode.Perm())
	}
}

func TestSyncRejectsInvalidBrokerAndPreservesOldState(t *testing.T) {
	dst := t.TempDir()
	old := filepath.Join(dst, "sentinel")
	if err := os.WriteFile(old, []byte("old-bytes"), 0o600); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(syncInvalidArchive(t))
	}))
	t.Cleanup(server.Close)

	if err := Sync(context.Background(), dst, server.URL); err == nil {
		t.Fatal("invalid broker unexpectedly installed")
	}
	got, err := os.ReadFile(old)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old-bytes" {
		t.Fatalf("old state changed to %q", got)
	}
}

func TestSyncRejectsNonLoopbackHTTP(t *testing.T) {
	if err := Sync(context.Background(), t.TempDir(), "http://example.test/registry.tar.gz"); err == nil {
		t.Fatal("non-loopback HTTP URL unexpectedly accepted")
	}
}

func syncValidArchive(t *testing.T) []byte {
	t.Helper()
	return makeArchive(t, []archiveEntry{
		{name: "manifest.json", body: `{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}`},
		{name: "schemas/broker.schema.json", body: `{"schema_version":1}`},
		{name: "brokers/us/test.yaml", body: "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"},
	})
}

func syncInvalidArchive(t *testing.T) []byte {
	t.Helper()
	return makeArchive(t, []archiveEntry{
		{name: "manifest.json", body: `{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}`},
		{name: "schemas/broker.schema.json", body: `{"schema_version":1}`},
		{name: "brokers/us/test.yaml", body: "id: different\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"},
	})
}

type archiveEntry struct {
	name string
	body string
}

func syncTestArchive(t *testing.T) []byte {
	t.Helper()
	return makeArchive(t, []archiveEntry{
		{name: "../../escape.txt", body: "escaped"},
		{name: "/absolute.txt", body: "absolute"},
		{name: "nested/safe..name.txt", body: "rejected"},
		{name: "nested/good.txt", body: "safe"},
	})
}

func makeArchive(t *testing.T, entries []archiveEntry) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for _, entry := range entries {
		if err := tw.WriteHeader(&tar.Header{
			Name:     entry.name,
			Mode:     0o600,
			Size:     int64(len(entry.body)),
			Typeflag: tar.TypeReg,
		}); err != nil {
			t.Fatal(err)
		}
		if _, err := tw.Write([]byte(entry.body)); err != nil {
			t.Fatal(err)
		}
	}
	if err := tw.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gz.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func fileMode(t *testing.T, path string) os.FileMode {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	return info.Mode()
}

func TestReplaceDirectoryUsesOwnedUniqueBackupAndCleansIt(t *testing.T) {
	parent := t.TempDir()
	dst := filepath.Join(parent, "registry")
	stage := filepath.Join(parent, "stage")
	sibling := dst + ".registry-backup"
	if err := os.MkdirAll(dst, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dst, "state"), []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(stage, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stage, "state"), []byte("new"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(sibling, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sibling, "sentinel"), []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}

	if err := replaceDirectory(dst, stage); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(filepath.Join(dst, "state")); err != nil || string(got) != "new" {
		t.Fatalf("installed state = %q, err=%v", got, err)
	}
	if got, err := os.ReadFile(filepath.Join(sibling, "sentinel")); err != nil || string(got) != "keep" {
		t.Fatalf("pre-existing sibling changed: %q, err=%v", got, err)
	}
	matches, err := filepath.Glob(filepath.Join(parent, ".registry-backup-*"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 0 {
		t.Fatalf("owned backup containers remain: %v", matches)
	}
}

func TestReplaceDirectoryInstallFailureRollsBackAndCleansOwnedBackup(t *testing.T) {
	parent := t.TempDir()
	dst := filepath.Join(parent, "registry")
	stage := filepath.Join(parent, "stage")
	if err := os.MkdirAll(dst, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dst, "state"), []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(stage, 0o700); err != nil {
		t.Fatal(err)
	}

	originalRename := renamePath
	t.Cleanup(func() { renamePath = originalRename })
	calls := 0
	renamePath = func(source, destination string) error {
		calls++
		if calls == 2 {
			return errors.New("forced install failure")
		}
		return originalRename(source, destination)
	}

	if err := replaceDirectory(dst, stage); err == nil {
		t.Fatal("forced install failure unexpectedly succeeded")
	}
	if got, err := os.ReadFile(filepath.Join(dst, "state")); err != nil || string(got) != "old" {
		t.Fatalf("rollback state = %q, err=%v", got, err)
	}
	matches, err := filepath.Glob(filepath.Join(parent, ".registry-backup-*"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 0 {
		t.Fatalf("owned backup containers remain after successful rollback: %v", matches)
	}
}

func TestReplaceDirectoryRollbackFailureRetainsRecoverableBackup(t *testing.T) {
	parent := t.TempDir()
	dst := filepath.Join(parent, "registry")
	stage := filepath.Join(parent, "stage")
	if err := os.MkdirAll(dst, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dst, "state"), []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(stage, 0o700); err != nil {
		t.Fatal(err)
	}

	originalRename := renamePath
	t.Cleanup(func() { renamePath = originalRename })
	calls := 0
	renamePath = func(source, destination string) error {
		calls++
		if calls >= 2 {
			return errors.New("forced replacement failure")
		}
		return originalRename(source, destination)
	}

	err := replaceDirectory(dst, stage)
	if err == nil || !strings.Contains(err.Error(), "backup retained") || !strings.Contains(err.Error(), "rollback failed") {
		t.Fatalf("rollback failure error = %v", err)
	}
	matches, err := filepath.Glob(filepath.Join(parent, ".registry-backup-*"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 1 {
		t.Fatalf("recoverable backup containers = %v", matches)
	}
	if got, err := os.ReadFile(filepath.Join(matches[0], "old", "state")); err != nil || string(got) != "old" {
		t.Fatalf("retained backup = %q, err=%v", got, err)
	}
	if err := os.RemoveAll(matches[0]); err != nil {
		t.Fatal(err)
	}
}
