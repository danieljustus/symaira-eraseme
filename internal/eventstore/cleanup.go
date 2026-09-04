// Package eventstore (cleanup.go) is the Go port of
// symeraseme.core.db_cleanup: stale temp-file scavenging, WAL
// checkpoint, file locking for encrypted DBs, and atexit-style
// re-encryption.  The Go version is goroutine-safe and does not
// rely on signal handlers — cleanup is driven by explicit Close
// calls plus explicit finalisation (the caller wires that to its
// own lifecycle).
package eventstore

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"time"
)

// StaleScavengeAge is the cutoff (mtime seconds) for stale
// decrypted-temp files.  Mirrors db_cleanup.STALE_SCAVENGE_AGE.
const StaleScavengeAge = 300 * time.Second

// ScavengeStaleTemps removes orphaned decrypted temp files older
// than StaleScavengeAge (and their WAL/-shm siblings).  Mirrors
// _scavenge_stale_temp_dbs.  No-op when the dir doesn't exist.
func ScavengeStaleTemps(tmpDir string) error {
	entries, err := os.ReadDir(tmpDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	now := time.Now()
	var files []os.DirEntry
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if !startsWith(e.Name(), "symeraseme_decrypted_") {
			continue
		}
		files = append(files, e)
	}
	sort.Slice(files, func(i, j int) bool { return files[i].Name() < files[j].Name() })
	for _, e := range files {
		full := filepath.Join(tmpDir, e.Name())
		info, err := e.Info()
		if err != nil {
			continue
		}
		if now.Sub(info.ModTime()) <= StaleScavengeAge {
			continue
		}
		_ = os.Remove(full)
		// WAL siblings use the temp-file's suffix (.db) → .db-wal etc.
		_ = os.Remove(full + "-wal")
		_ = os.Remove(full + "-shm")
	}
	return nil
}

func startsWith(s, prefix string) bool {
	if len(s) < len(prefix) {
		return false
	}
	return s[:len(prefix)] == prefix
}

// CheckpointWAL runs a PRAGMA wal_checkpoint(TRUNCATE) on the
// given file to flush pending writes into the main DB before
// re-encrypting. SQLite reports a busy checkpoint in the first result
// column, not as a Go error, so all result columns are inspected.
func (s *Store) CheckpointWAL() error {
	var busy, logFrames, checkpointed int64
	row := s.db.QueryRow("PRAGMA wal_checkpoint(TRUNCATE)")
	if err := row.Scan(&busy, &logFrames, &checkpointed); err != nil {
		return fmt.Errorf("eventstore: WAL checkpoint: %w", err)
	}
	if busy != 0 {
		return fmt.Errorf("eventstore: WAL checkpoint busy (busy=%d log_frames=%d checkpointed=%d)", busy, logFrames, checkpointed)
	}
	return nil
}

// RemoveWALSiblings deletes the .db-wal and .db-shm files next to
// the given path.  Mirrors _checkpoint_and_cleanup_wal.
func RemoveWALSiblings(path string) error {
	for _, suffix := range []string{"-wal", "-shm"} {
		sib := path + suffix
		if err := os.Remove(sib); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
	}
	return nil
}

// --------------------------------------------------------------------
// File locking (encrypted-DB mutual exclusion)
// --------------------------------------------------------------------

// DBLock is an exclusive lock on a sibling .lock file.  Released
// by Close.  No-op on platforms where flock is unavailable.
type DBLock struct {
	path string
	file *os.File
}

// LockDB acquires an exclusive lock at dbPath+".lock".  Returns
// nil when the lock could not be acquired (or the platform has no
// flock) — encryption callers treat the absence of a lock as a
// best-effort.  retryMax controls the number of attempts.
func LockDB(dbPath string, retryMax int) (*DBLock, error) {
	lockPath := dbPath + ".lock"
	attempts := retryMax
	if attempts <= 0 {
		attempts = 1
	}
	var lastErr error
	for i := 0; i < attempts; i++ {
		f, err := os.OpenFile(lockPath, os.O_RDWR|os.O_CREATE, 0o600) //nolint:gosec // our own lock file
		if err != nil {
			lastErr = err
			time.Sleep(time.Second)
			continue
		}
		// Try non-blocking exclusive lock.  flock is a no-op on
		// Windows; we accept that the file-exists check is the
		// best we can do there.
		if err := flockExclusive(f); err != nil {
			_ = f.Close()
			lastErr = err
			time.Sleep(time.Second)
			continue
		}
		return &DBLock{path: lockPath, file: f}, nil
	}
	return nil, fmt.Errorf("eventstore: cannot acquire DB lock %s: %w", lockPath, lastErr)
}

// Close releases the lock.  Safe to call on a nil receiver.
func (l *DBLock) Close() error {
	if l == nil || l.file == nil {
		return nil
	}
	defer func() { l.file = nil }()
	funlock(l.file)
	return l.file.Close()
}

// --------------------------------------------------------------------
// Encrypted re-encrypt on exit
// --------------------------------------------------------------------

// FinaliseAll finalizes every still-registered encrypted store. Entries
// registered by OpenEncrypted carry their Store owner, so finalization
// checkpoints and closes SQLite before reading the temp file. Low-level
// RegisterTemp entries are intended for already-closed databases and are
// written directly. All failures are returned and failed entries remain
// registered with their recovery artifacts intact.
func FinaliseAll() error {
	var finaliseErr error
	encryptedTemps.Range(func(k, v any) bool {
		origPath, _ := k.(string)
		reg, ok := tempRegistration(v)
		if origPath == "" || !ok {
			return true
		}
		var err error
		if reg.store != nil {
			err = reg.store.CloseAt(origPath)
		} else {
			err = WriteEncrypted(origPath)
		}
		if err != nil {
			finaliseErr = errors.Join(finaliseErr, fmt.Errorf("%s: %w", origPath, err))
		}
		return true
	})
	return finaliseErr
}

// EncryptAndClose re-encrypts the Store's temp file back to
// encPath, then closes the connection. Convenience wrapper used
// by callers that opened via OpenEncrypted.
func (s *Store) EncryptAndClose(encPath string) error {
	return s.CloseAt(encPath)
}

// --------------------------------------------------------------------
// Conformance helper: Apply / re-derive all dirty projections
// --------------------------------------------------------------------

// ApplyPendingProjects is a convenience wrapper for
// Store.RebuildAllStates.  It exists so callers can use one
// "cleanup"-flavoured entry point.
func (s *Store) ApplyPendingProjects(ctx context.Context) (int, error) {
	return s.RebuildAllStates(ctx, 100)
}
