package main

import (
	"bytes"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func TestCLIDataStoreUsesConfiguredDatabaseAndEncryption(t *testing.T) {
	dataDir := t.TempDir()
	dbDir := t.TempDir()
	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("SYMERASEME_DB_DIR", dbDir)
	t.Setenv("SYMERASEME_ENCRYPT_DB", "true")

	master := bytes.Repeat([]byte{0x29}, 32)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", hex.EncodeToString(master))
	identity.Shutdown()
	t.Cleanup(identity.Shutdown)

	store, err := dataStore()
	if err != nil {
		t.Fatalf("dataStore encrypted = %v", err)
	}
	if store.Path() != filepath.Join(dbDir, eventstore.DBFileName) {
		t.Fatalf("store path = %q, want database override", store.Path())
	}
	if _, err := store.CreateCampaign(t.Context(), "configured", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("encrypted close = %v", err)
	}
	raw, err := os.ReadFile(filepath.Join(dbDir, eventstore.DBFileName))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(raw, eventstore.EncMagicV3) {
		t.Fatalf("configured encrypted database has header %q", raw[:min(len(raw), len(eventstore.EncMagicV3))])
	}

	// The same production resolver must support an explicit downgrade without
	// losing the campaign written during the encrypted run. Clear process-local
	// key state first to model a fresh CLI process.
	identity.Shutdown()
	t.Setenv("SYMERASEME_ENCRYPT_DB", "false")
	store, err = dataStore()
	if err != nil {
		t.Fatalf("dataStore plaintext = %v", err)
	}
	created, err := store.CreateCampaign(t.Context(), "configured", "initial", "")
	if err != nil {
		t.Fatal(err)
	}
	if created {
		t.Fatal("encrypted data was not recovered before plaintext open")
	}
	if err := store.Close(); err != nil {
		t.Fatalf("plaintext close = %v", err)
	}
	raw, err = os.ReadFile(filepath.Join(dbDir, eventstore.DBFileName))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.HasPrefix(raw, eventstore.EncMagicV3) {
		t.Fatal("encryption-disabled database retained encrypted header")
	}
}

func TestLoadRegistryHonorsConfigResources(t *testing.T) {
	resDir := t.TempDir()
	t.Setenv("SYMERASEME_RESOURCES", resDir)
	manifest := `schema_version = "1.0.0"
manifest_version = "1.0.0"
generated_at = "2026-01-01T00:00:00Z"
broker_count = 0
`
	if err := os.WriteFile(filepath.Join(resDir, "manifest.toml"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	brokers, err := loadRegistry()
	if err != nil {
		t.Fatalf("loadRegistry with custom empty dir error: %v", err)
	}
	if len(brokers) != 0 {
		t.Fatalf("brokers len = %d, want 0", len(brokers))
	}
}
