package registry

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
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
