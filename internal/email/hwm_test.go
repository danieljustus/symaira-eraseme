package email_test

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func TestDBHWMStorePersistenceAcrossInstances(t *testing.T) {
	ctx := context.Background()
	dbPath := filepath.Join(t.TempDir(), "symeraseme.db")

	store1, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open store1: %v", err)
	}

	hwm1 := store1

	// Initially empty
	v, u, err := hwm1.Get(ctx, "imap.example.com", "INBOX")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if v != nil || u != nil {
		t.Fatalf("expected nil for empty, got v=%v, u=%v", v, u)
	}

	// Set value
	if err := hwm1.Set(ctx, "imap.example.com", "INBOX", 100, 50); err != nil {
		t.Fatalf("Set: %v", err)
	}

	// Read back
	v, u, err = hwm1.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || v == nil || *v != 100 || u == nil || *u != 50 {
		t.Fatalf("unexpected Get: v=%v, u=%v, err=%v", v, u, err)
	}

	// Close store1
	_ = store1.Close()

	// New process/service instance opens the DB
	store2, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open store2: %v", err)
	}
	defer store2.Close()

	hwm2 := store2
	v, u, err = hwm2.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || v == nil || *v != 100 || u == nil || *u != 50 {
		t.Fatalf("unexpected Get on instance 2: v=%v, u=%v, err=%v", v, u, err)
	}

	// Update on instance 2
	if err := hwm2.Set(ctx, "imap.example.com", "INBOX", 100, 75); err != nil {
		t.Fatalf("Set on instance 2: %v", err)
	}
	v, u, err = hwm2.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || v == nil || *v != 100 || u == nil || *u != 75 {
		t.Fatalf("unexpected Get after update on instance 2: v=%v, u=%v, err=%v", v, u, err)
	}
}

func TestStagingHWMStore(t *testing.T) {
	ctx := context.Background()
	underlying := email.NewMemoryHWMStore()
	staged := email.NewStagingHWMStore(underlying)

	// Initially empty
	v, u, err := staged.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || v != nil || u != nil {
		t.Fatalf("expected empty initially, got v=%v, u=%v, err=%v", v, u, err)
	}

	// Stage an update
	if err := staged.Set(ctx, "imap.example.com", "INBOX", 10, 100); err != nil {
		t.Fatalf("Set failed: %v", err)
	}

	// Staged store sees updated value
	v, u, err = staged.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || v == nil || *v != 10 || u == nil || *u != 100 {
		t.Fatalf("staged Get failed: v=%v, u=%v, err=%v", v, u, err)
	}

	// Underlying store does NOT see updated value before commit
	vUnderlying, uUnderlying, err := underlying.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || vUnderlying != nil || uUnderlying != nil {
		t.Fatalf("underlying should not see staged update before commit: v=%v, u=%v", vUnderlying, uUnderlying)
	}

	// Commit staged updates
	if err := staged.Commit(ctx); err != nil {
		t.Fatalf("Commit failed: %v", err)
	}

	// Now underlying sees updated value
	vUnderlying, uUnderlying, err = underlying.Get(ctx, "imap.example.com", "INBOX")
	if err != nil || vUnderlying == nil || *vUnderlying != 10 || uUnderlying == nil || *uUnderlying != 100 {
		t.Fatalf("underlying should see updated value after commit: v=%v, u=%v", vUnderlying, uUnderlying)
	}
}
