package eventstore

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestScavengeStaleTempsRemovesOnlyOldDatabaseArtifacts(t *testing.T) {
	dir := t.TempDir()
	stale := filepath.Join(dir, "symeraseme_decrypted_old.db")
	staleWrite := filepath.Join(dir, ".symeraseme_write_crashed.tmp")
	recent := filepath.Join(dir, "symeraseme_decrypted_recent.db")
	ignored := filepath.Join(dir, "other.db")
	for _, path := range []string{stale, stale + "-wal", stale + "-shm", staleWrite, recent, ignored} {
		if err := os.WriteFile(path, []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	old := time.Now().Add(-StaleScavengeAge - time.Second)
	if err := os.Chtimes(stale, old, old); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(staleWrite, old, old); err != nil {
		t.Fatal(err)
	}
	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{stale, stale + "-wal", stale + "-shm", staleWrite} {
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			t.Errorf("stale artifact %s remains", path)
		}
	}
	for _, path := range []string{recent, ignored} {
		if _, err := os.Stat(path); err != nil {
			t.Errorf("non-stale artifact %s was removed: %v", path, err)
		}
	}
	if _, err := os.Stat(recent + ".lock"); !os.IsNotExist(err) {
		t.Errorf("fresh temp acquired an orphan lock sidecar: %v", err)
	}
	if err := ScavengeStaleTemps(filepath.Join(dir, "missing")); err != nil {
		t.Fatal(err)
	}
}

func TestScavengeStaleTempsSkipsLockedDecryptedTemp(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "symeraseme_decrypted_active.db")
	for _, candidate := range []string{path, path + "-wal", path + "-shm"} {
		if err := os.WriteFile(candidate, []byte("active"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	old := time.Now().Add(-StaleScavengeAge - time.Second)
	if err := os.Chtimes(path, old, old); err != nil {
		t.Fatal(err)
	}
	lock, err := LockDB(path, 1)
	if err != nil {
		t.Fatal(err)
	}
	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range []string{path, path + "-wal", path + "-shm"} {
		if _, err := os.Stat(candidate); err != nil {
			t.Fatalf("locked stale artifact %s was scavenged: %v", candidate, err)
		}
	}
	if err := lock.Close(); err != nil {
		t.Fatal(err)
	}
	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("unlocked stale temp remains: %v", err)
	}
	if _, err := os.Stat(path + ".lock"); !os.IsNotExist(err) {
		t.Fatalf("temp lock sidecar remains after scavenging: %v", err)
	}
}

func TestScavengeStaleTempsDoesNotScavengeLockSidecars(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "symeraseme_decrypted_orphan.db.lock")
	if err := os.WriteFile(path, []byte{}, 0o600); err != nil {
		t.Fatal(err)
	}
	old := time.Now().Add(-StaleScavengeAge - time.Second)
	if err := os.Chtimes(path, old, old); err != nil {
		t.Fatal(err)
	}
	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("lock sidecar was scavenged: %v", err)
	}
}

func TestScavengeStaleTempsRetainsTempWhenSiblingCleanupFails(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "symeraseme_decrypted_retry.db")
	for _, candidate := range []string{path, path + "-wal", path + "-shm"} {
		if err := os.WriteFile(candidate, []byte("stale"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	old := time.Now().Add(-StaleScavengeAge - time.Second)
	if err := os.Chtimes(path, old, old); err != nil {
		t.Fatal(err)
	}
	cleanupErr := errors.New("sibling cleanup failure")
	oldRemoveWAL := removeWALSiblingsFn
	removeWALSiblingsFn = func(string) error { return cleanupErr }
	t.Cleanup(func() { removeWALSiblingsFn = oldRemoveWAL })

	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range []string{path, path + "-wal", path + "-shm", path + ".lock"} {
		if _, err := os.Stat(candidate); err != nil {
			t.Fatalf("artifact %s was not retained after cleanup failure: %v", candidate, err)
		}
	}

	removeWALSiblingsFn = RemoveWALSiblings
	if err := ScavengeStaleTemps(dir); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range []string{path, path + "-wal", path + "-shm", path + ".lock"} {
		if _, err := os.Stat(candidate); !os.IsNotExist(err) {
			t.Fatalf("artifact %s remains after retry: %v", candidate, err)
		}
	}
}

func TestRemoveWALSiblingsAndLockDB(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "db")
	for _, suffix := range []string{"-wal", "-shm"} {
		if err := os.WriteFile(path+suffix, []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := RemoveWALSiblings(path); err != nil {
		t.Fatal(err)
	}
	if err := RemoveWALSiblings(path); err != nil {
		t.Fatal(err)
	}
	lock, err := LockDB(path, 1)
	if err != nil || lock == nil {
		t.Fatalf("LockDB = %#v, err=%v", lock, err)
	}
	if err := lock.Close(); err != nil {
		t.Fatal(err)
	}
	if err := (*DBLock)(nil).Close(); err != nil {
		t.Fatal(err)
	}
}

func TestStoreApplyPendingProjectsWrapper(t *testing.T) {
	store, err := Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "broker", "email", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(ctx, requestID, EvtPlanned, nil, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	if count, err := store.ApplyPendingProjects(ctx); err != nil || count != 1 {
		t.Fatalf("ApplyPendingProjects = %d, err=%v", count, err)
	}
	if count, err := store.ApplyPendingProjects(ctx); err != nil || count != 0 {
		t.Fatalf("second ApplyPendingProjects = %d, err=%v", count, err)
	}
}
