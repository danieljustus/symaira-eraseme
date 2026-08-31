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

func TestSyncRejectsArchiveTraversal(t *testing.T) {
	dst := t.TempDir()
	outside := filepath.Join(filepath.Dir(dst), "escape.txt")
	if err := os.Remove(outside); err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/gzip")
		_, _ = w.Write(syncTestArchive(t))
	}))
	t.Cleanup(server.Close)

	if err := Sync(context.Background(), dst, server.URL); err != nil {
		t.Fatal(err)
	}

	if _, err := os.Stat(outside); !os.IsNotExist(err) {
		t.Fatalf("archive traversal created %s: %v", outside, err)
	}
	if _, err := os.Stat(filepath.Join(dst, "absolute.txt")); !os.IsNotExist(err) {
		t.Fatalf("absolute archive path was extracted: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "nested", "safe..name.txt")); !os.IsNotExist(err) {
		t.Fatalf("archive path containing '..' was extracted: %v", err)
	}
	good, err := os.ReadFile(filepath.Join(dst, "nested", "good.txt"))
	if err != nil {
		t.Fatalf("safe archive entry missing: %v", err)
	}
	if string(good) != "safe" {
		t.Fatalf("safe archive entry = %q", good)
	}
}

func syncTestArchive(t *testing.T) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	entries := []struct {
		name string
		body string
	}{
		{name: "../../escape.txt", body: "escaped"},
		{name: "/absolute.txt", body: "absolute"},
		{name: "nested/safe..name.txt", body: "rejected"},
		{name: "nested/good.txt", body: "safe"},
	}
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
