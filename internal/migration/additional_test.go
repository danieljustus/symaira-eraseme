package migration

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestMetadataOnlySecretStoreDetectionAndMigrationBoundary(t *testing.T) {
	store := MetadataOnlySecretStore{}
	dir := t.TempDir()
	if report, err := store.Inspect(context.Background(), dir); err != nil || report.Detected {
		t.Fatalf("empty secret store = %#v, err=%v", report, err)
	}
	if err := os.WriteFile(filepath.Join(dir, "identity.enc"), []byte("metadata"), 0o600); err != nil {
		t.Fatal(err)
	}
	report, err := store.Inspect(context.Background(), dir)
	if err != nil || !report.Detected || report.Migratable || report.Description == "" {
		t.Fatalf("detected secret store = %#v, err=%v", report, err)
	}
	if err := store.Migrate(context.Background(), dir); err == nil {
		t.Fatal("metadata-only store migrated secrets")
	}
}
