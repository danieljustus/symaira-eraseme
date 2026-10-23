package mcp

import (
	"bytes"
	"context"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func TestMCPDataStoreUsesConfiguredDatabaseAndEncryption(t *testing.T) {
	dataDir := t.TempDir()
	dbDir := t.TempDir()
	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("SYMERASEME_DB_DIR", dbDir)
	t.Setenv("SYMERASEME_ENCRYPT_DB", "true")

	master := bytes.Repeat([]byte{0x33}, 32)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", hex.EncodeToString(master))
	identity.Shutdown()
	t.Cleanup(identity.Shutdown)

	store, err := dataStore()
	if err != nil {
		t.Fatalf("mcp dataStore encrypted = %v", err)
	}
	if store.Path() != filepath.Join(dbDir, eventstore.DBFileName) {
		t.Fatalf("store path = %q, want database override", store.Path())
	}
	if _, err := store.CreateCampaign(context.Background(), "mcp-configured", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("mcp encrypted close = %v", err)
	}
	raw, err := os.ReadFile(filepath.Join(dbDir, eventstore.DBFileName))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(raw, eventstore.EncMagicV3) {
		t.Fatalf("configured encrypted database has header %q", raw[:min(len(raw), len(eventstore.EncMagicV3))])
	}

	// Downgrade to plaintext from a fresh-process key state.
	identity.Shutdown()
	t.Setenv("SYMERASEME_ENCRYPT_DB", "false")
	store, err = dataStore()
	if err != nil {
		t.Fatalf("mcp dataStore plaintext = %v", err)
	}
	created, err := store.CreateCampaign(context.Background(), "mcp-configured", "initial", "")
	if err != nil {
		t.Fatal(err)
	}
	if created {
		t.Fatal("encrypted campaign was not recovered before plaintext open")
	}
	if err := store.Close(); err != nil {
		t.Fatalf("mcp plaintext close = %v", err)
	}
	raw, err = os.ReadFile(filepath.Join(dbDir, eventstore.DBFileName))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.HasPrefix(raw, eventstore.EncMagicV3) {
		t.Fatal("encryption-disabled database retained encrypted header")
	}
}

func TestMCPContractHandlerHonorsConfiguredStorage(t *testing.T) {
	dataDir := t.TempDir()
	dbDir := t.TempDir()
	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("SYMERASEME_DB_DIR", dbDir)
	t.Setenv("SYMERASEME_ENCRYPT_DB", "true")

	master := bytes.Repeat([]byte{0x44}, 32)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", hex.EncodeToString(master))
	identity.Shutdown()
	t.Cleanup(identity.Shutdown)

	ctx := context.Background()
	handler := ContractHandler()
	res, err := handler(ctx, "plan_create", map[string]any{
		"campaign_id": "mcp-handler-test",
		"max_brokers": 1,
	})
	if err != nil {
		t.Fatalf("plan_create failed: %v", err)
	}
	if res == nil {
		t.Fatal("plan_create returned nil result")
	}

	dbFile := filepath.Join(dbDir, eventstore.DBFileName)
	raw, err := os.ReadFile(dbFile)
	if err != nil {
		t.Fatalf("expected db file at %s: %v", dbFile, err)
	}
	if !bytes.HasPrefix(raw, eventstore.EncMagicV3) {
		t.Fatalf("expected encrypted header on db file, got %q", raw[:min(len(raw), len(eventstore.EncMagicV3))])
	}
}
