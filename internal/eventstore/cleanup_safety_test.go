package eventstore

import (
	"bytes"
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

var registerCheckpointDrivers sync.Once

// checkpointDriver is a deterministic database/sql driver used to exercise
// the SQLite checkpoint result-row contract without relying on host timing or
// filesystem locking behavior.
type checkpointDriver struct {
	busy    int64
	log     int64
	flushed int64
	err     error
}

func (d checkpointDriver) Open(string) (driver.Conn, error) {
	return checkpointConn{driver: d}, nil
}

type checkpointConn struct{ driver checkpointDriver }

func (checkpointConn) Prepare(string) (driver.Stmt, error) {
	return nil, errors.New("prepare not supported")
}
func (checkpointConn) Close() error              { return nil }
func (checkpointConn) Begin() (driver.Tx, error) { return nil, errors.New("begin not supported") }

func (c checkpointConn) QueryContext(context.Context, string, []driver.NamedValue) (driver.Rows, error) {
	if c.driver.err != nil {
		return nil, c.driver.err
	}
	return &checkpointRows{
		values: []driver.Value{c.driver.busy, c.driver.log, c.driver.flushed},
	}, nil
}

type checkpointRows struct {
	values []driver.Value
	read   bool
}

func (*checkpointRows) Columns() []string { return []string{"busy", "log", "checkpointed"} }
func (*checkpointRows) Close() error      { return nil }
func (r *checkpointRows) Next(dest []driver.Value) error {
	if r.read {
		return io.EOF
	}
	copy(dest, r.values)
	r.read = true
	return nil
}

func initCheckpointDrivers() {
	registerCheckpointDrivers.Do(func() {
		sql.Register("eventstore_test_checkpoint_busy", checkpointDriver{busy: 1, log: 4, flushed: 0})
		sql.Register("eventstore_test_checkpoint_ok", checkpointDriver{busy: 0, log: 0, flushed: 0})
		sql.Register("eventstore_test_checkpoint_error", checkpointDriver{err: errors.New("checkpoint I/O failure")})
	})
}

func openCheckpointStore(t *testing.T, driverName string) *Store {
	t.Helper()
	initCheckpointDrivers()
	db, err := sql.Open(driverName, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return &Store{db: db, path: filepath.Join(t.TempDir(), "db")}
}

func writeRecoveryFixture(t *testing.T) (orig, tmp string, before []byte) {
	t.Helper()
	dir := t.TempDir()
	orig = filepath.Join(dir, "encrypted.db")
	tmp = filepath.Join(dir, "symeraseme_decrypted_test.db")
	before = []byte("original encrypted bytes")
	if err := os.WriteFile(orig, before, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(tmp, []byte("recoverable sqlite bytes"), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if err := os.WriteFile(tmp+suffix, []byte("recoverable "+suffix), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	RegisterTemp(orig, tmp)
	t.Cleanup(func() { ForgetTemp(orig) })
	return orig, tmp, before
}

func restoreCloseHooks(t *testing.T) {
	t.Helper()
	oldClose := closeStoreDBFn
	oldAtomic := atomicWriteFileFn
	oldReplace := replaceFileFn
	oldSyncDir := syncDirectoryFn
	oldRemoveWAL := removeWALSiblingsFn
	oldCheckpoint := checkpointWALFn
	t.Cleanup(func() {
		closeStoreDBFn = oldClose
		atomicWriteFileFn = oldAtomic
		replaceFileFn = oldReplace
		syncDirectoryFn = oldSyncDir
		removeWALSiblingsFn = oldRemoveWAL
		checkpointWALFn = oldCheckpoint
	})
}

func TestCheckpointWALRejectsBusyResultRow(t *testing.T) {
	store := openCheckpointStore(t, "eventstore_test_checkpoint_busy")
	err := store.CheckpointWAL()
	if err == nil || !strings.Contains(err.Error(), "busy") {
		t.Fatalf("busy checkpoint error = %v, want busy error", err)
	}
}

func TestCloseAtRetriesCheckpointBeforeEncrypting(t *testing.T) {
	master := bytes32(0x77)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	encPath := filepath.Join(dir, "retry.db")
	store, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp"))
	if err != nil {
		t.Fatal(err)
	}
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker", "email", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(context.Background(), requestID, EvtSent, map[string]any{"retry": true}, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}

	var checkpointCalls int
	checkpointWALFn = func(s *Store) error {
		checkpointCalls++
		if checkpointCalls == 1 {
			return errors.New("injected checkpoint failure")
		}
		return s.CheckpointWAL()
	}
	if err := store.CloseAt(encPath); err == nil || !strings.Contains(err.Error(), "injected checkpoint failure") {
		t.Fatalf("first CloseAt error = %v, want injected checkpoint failure", err)
	}
	if store.dbClosed {
		t.Fatal("failed checkpoint marked the database closed")
	}
	if _, err := store.GetEvents(context.Background(), requestID, 0); err != nil {
		t.Fatalf("database unusable after failed checkpoint: %v", err)
	}
	if err := store.CloseAt(encPath); err != nil {
		t.Fatalf("retry CloseAt = %v", err)
	}
	if checkpointCalls != 2 {
		t.Fatalf("checkpoint calls = %d, want 2", checkpointCalls)
	}

	store2, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp2"))
	if err != nil {
		t.Fatal(err)
	}
	defer store2.Close()
	events, err := store2.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].EventType != EvtSent {
		t.Fatalf("events after checkpoint retry = %+v, want one sent event", events)
	}
}

func TestCheckpointWALPropagatesQueryError(t *testing.T) {
	store := openCheckpointStore(t, "eventstore_test_checkpoint_error")
	if err := store.CheckpointWAL(); err == nil || !strings.Contains(err.Error(), "I/O failure") {
		t.Fatalf("checkpoint error = %v, want query error", err)
	}
}

func TestCloseAtBusyCheckpointPreservesRecoveryState(t *testing.T) {
	restoreCloseHooks(t)
	orig, tmp, before := writeRecoveryFixture(t)
	store := openCheckpointStore(t, "eventstore_test_checkpoint_busy")

	if err := store.CloseAt(orig); err == nil {
		t.Fatal("CloseAt accepted a busy checkpoint")
	}
	assertRecoveryState(t, orig, tmp, before)
}

func TestCloseAtCloseFailurePreservesRecoveryState(t *testing.T) {
	restoreCloseHooks(t)
	orig, tmp, before := writeRecoveryFixture(t)
	store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")
	closeErr := errors.New("database close failure")
	closeStoreDBFn = func(*Store) error { return closeErr }

	err := store.CloseAt(orig)
	if !errors.Is(err, closeErr) {
		t.Fatalf("CloseAt error = %v, want %v", err, closeErr)
	}
	assertRecoveryState(t, orig, tmp, before)
}

func TestCloseAtReadEncryptAndAtomicFailuresPreserveRecoveryState(t *testing.T) {
	for _, tc := range []struct {
		name     string
		prepare  func(t *testing.T, tmp string)
		wantErr  string
		atomicFn func(string, []byte, os.FileMode) error
	}{
		{
			name: "read",
			prepare: func(t *testing.T, tmp string) {
				if err := os.Remove(tmp); err != nil {
					t.Fatal(err)
				}
				if err := os.Mkdir(tmp, 0o700); err != nil {
					t.Fatal(err)
				}
			},
			wantErr: "is a directory",
		},
		{
			name:    "encrypt",
			prepare: func(*testing.T, string) { SetMasterKeyProvider(nil) },
			wantErr: "master key",
		},
		{
			name:    "atomic write",
			prepare: func(*testing.T, string) {},
			wantErr: "atomic write failure",
			atomicFn: func(string, []byte, os.FileMode) error {
				return errors.New("atomic write failure")
			},
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			restoreCloseHooks(t)
			master := bytes32(0x53)
			SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
			t.Cleanup(func() { SetMasterKeyProvider(nil) })
			orig, tmp, before := writeRecoveryFixture(t)
			tc.prepare(t, tmp)
			if tc.atomicFn != nil {
				atomicWriteFileFn = tc.atomicFn
			}
			store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")

			err := store.CloseAt(orig)
			if err == nil || !strings.Contains(strings.ToLower(err.Error()), strings.ToLower(tc.wantErr)) {
				t.Fatalf("CloseAt error = %v, want %q", err, tc.wantErr)
			}
			assertRecoveryState(t, orig, tmp, before)
		})
	}
}

func TestCloseAtSuccessCleansRecoveryArtifactsAfterAtomicWrite(t *testing.T) {
	restoreCloseHooks(t)
	master := bytes32(0x61)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	orig, tmp, _ := writeRecoveryFixture(t)
	store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")

	if err := store.CloseAt(orig); err != nil {
		t.Fatalf("CloseAt = %v", err)
	}
	if _, err := os.Stat(tmp); !os.IsNotExist(err) {
		t.Fatalf("temp remains after successful CloseAt: %v", err)
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if _, err := os.Stat(tmp + suffix); !os.IsNotExist(err) {
			t.Fatalf("temp %s remains after successful CloseAt: %v", suffix, err)
		}
	}
	if got, err := DecryptFile(orig); err != nil || string(got) != "recoverable sqlite bytes" {
		t.Fatalf("encrypted replacement = %q, err=%v", got, err)
	}
	if _, ok := encryptedTemps.Load(orig); ok {
		t.Fatal("successful CloseAt left temp registration")
	}
}

func TestFinaliseAllReturnsWriteFailureAndPreservesRegistration(t *testing.T) {
	orig, tmp, _ := writeRecoveryFixture(t)
	_ = tmp
	if err := FinaliseAll(); err == nil {
		t.Fatal("FinaliseAll swallowed a write failure")
	}
	if _, ok := encryptedTemps.Load(orig); !ok {
		t.Fatal("FinaliseAll discarded a failed registration")
	}
}

func assertRecoveryState(t *testing.T, orig, tmp string, before []byte) {
	t.Helper()
	if got, err := os.ReadFile(orig); err != nil || string(got) != string(before) {
		t.Fatalf("original changed on failed close: %q, err=%v", got, err)
	}
	if _, err := os.Stat(tmp); err != nil {
		t.Fatalf("recoverable temp missing after failed close: %v", err)
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if _, err := os.Stat(tmp + suffix); err != nil {
			t.Fatalf("recoverable temp %s missing after failed close: %v", suffix, err)
		}
	}
}

func bytes32(value byte) []byte {
	out := make([]byte, 32)
	for i := range out {
		out[i] = value
	}
	return out
}

func TestAtomicWriteFileReplacementFailurePreservesDestination(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "replacement.db")
	if err := os.WriteFile(path, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	restoreCloseHooks(t)
	replacementErr := errors.New("replacement failure")
	replaceFileFn = func(string, string) error { return replacementErr }
	if err := atomicWriteFile(path, []byte("new"), 0o600); !errors.Is(err, replacementErr) {
		t.Fatalf("atomicWriteFile error = %v, want %v", err, replacementErr)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old" {
		t.Fatalf("destination after failed replacement = %q, want old", got)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".symeraseme_write_") {
			t.Fatalf("temporary replacement file remains: %s", entry.Name())
		}
	}
}

func TestAtomicWriteFileSyncsContainingDirectory(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "replacement.db")
	if err := os.WriteFile(path, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	restoreCloseHooks(t)
	var synced string
	syncDirectoryFn = func(got string) error {
		synced = got
		return nil
	}
	if err := atomicWriteFile(path, []byte("new"), 0o600); err != nil {
		t.Fatalf("atomicWriteFile = %v", err)
	}
	if synced != dir {
		t.Fatalf("synced directory = %q, want %q", synced, dir)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "new" {
		t.Fatalf("replacement contents = %q, want new", got)
	}
}

func TestAtomicWriteFileDirectorySyncFailureIsReported(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "replacement.db")
	if err := os.WriteFile(path, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	restoreCloseHooks(t)
	syncErr := errors.New("directory sync failure")
	syncDirectoryFn = func(string) error { return syncErr }
	if err := atomicWriteFile(path, []byte("new"), 0o600); !errors.Is(err, syncErr) {
		t.Fatalf("atomicWriteFile error = %v, want %v", err, syncErr)
	}
}

func TestFinaliseAllClosesOwnedStoreBeforeEncryption(t *testing.T) {
	master := bytes32(0x72)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	encPath := filepath.Join(dir, "finalise.db")
	store, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp"))
	if err != nil {
		t.Fatal(err)
	}
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker", "email", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(context.Background(), requestID, EvtSent, map[string]any{"round": 1}, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	if err := FinaliseAll(); err != nil {
		t.Fatalf("FinaliseAll = %v", err)
	}
	// FinaliseAll owns the store close; a deferred Close must not attempt a
	// second checkpoint or database close.
	if err := store.Close(); err != nil {
		t.Fatalf("second Close = %v", err)
	}
	store2, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp2"))
	if err != nil {
		t.Fatal(err)
	}
	defer store2.Close()
	events, err := store2.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("events after FinaliseAll = %d, want 1", len(events))
	}
	if err := store2.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestWriteEncryptedClosesOwnedStoreBeforeReading(t *testing.T) {
	master := bytes32(0x76)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	encPath := filepath.Join(dir, "write-encrypted.db")
	store, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp"))
	if err != nil {
		t.Fatal(err)
	}
	requestID, err := store.CreateRemovalRequest(context.Background(), "broker", "email", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(context.Background(), requestID, EvtSent, map[string]any{"round": 1}, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	if err := WriteEncrypted(encPath); err != nil {
		t.Fatalf("WriteEncrypted = %v", err)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("second Close = %v", err)
	}
	store2, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp2"))
	if err != nil {
		t.Fatal(err)
	}
	events, err := store2.GetEvents(context.Background(), requestID, 0)
	if err != nil {
		store2.Close()
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("events after WriteEncrypted = %d, want 1", len(events))
	}
	if err := store2.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestPlainCloseRemovesWALSiblingsOnlyAfterEncryption(t *testing.T) {
	master := bytes32(0x73)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	encPath := filepath.Join(dir, "plain.db")
	store, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp"))
	if err != nil {
		t.Fatal(err)
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if err := os.WriteFile(encPath+suffix, []byte("sidecar"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	var removed string
	removeWALSiblingsFn = func(path string) error {
		removed = path
		return RemoveWALSiblings(path)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("Close = %v", err)
	}
	if removed != encPath {
		t.Fatalf("WAL cleanup path = %q, want %q", removed, encPath)
	}
	encrypted, err := IsEncrypted(encPath)
	if err != nil || !encrypted {
		t.Fatalf("plain close did not produce an encrypted file: encrypted=%v, err=%v", encrypted, err)
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if _, err := os.Stat(encPath + suffix); !os.IsNotExist(err) {
			t.Fatalf("sidecar %s remains after successful close: %v", suffix, err)
		}
	}
}

func TestPlainCloseEncryptionFailureLeavesWALSiblings(t *testing.T) {
	master := bytes32(0x74)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	encPath := filepath.Join(dir, "plain-failure.db")
	if err := os.WriteFile(encPath, []byte("plain sqlite"), 0o600); err != nil {
		t.Fatal(err)
	}
	store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")
	RegisterTemp(encPath, encPath)
	t.Cleanup(func() { ForgetTemp(encPath) })
	for _, suffix := range []string{"-wal", "-shm"} {
		if err := os.WriteFile(encPath+suffix, []byte("sidecar"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	atomicWriteFileFn = func(string, []byte, os.FileMode) error {
		return errors.New("replacement failure")
	}
	removed := false
	removeWALSiblingsFn = func(string) error {
		removed = true
		return nil
	}
	if err := store.CloseAt(encPath); err == nil || !strings.Contains(err.Error(), "replacement failure") {
		t.Fatalf("Close error = %v, want replacement failure", err)
	}
	if removed {
		t.Fatal("WAL siblings were removed after encryption failure")
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		if _, err := os.Stat(encPath + suffix); err != nil {
			t.Fatalf("sidecar %s missing after failed close: %v", suffix, err)
		}
	}
}

func TestPlainCloseDirectorySyncFailureKeepsRecoveryCopy(t *testing.T) {
	master := bytes32(0x75)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	encPath := filepath.Join(dir, "plain-sync-failure.db")
	plain := []byte("plain sqlite recovery source")
	if err := os.WriteFile(encPath, plain, 0o600); err != nil {
		t.Fatal(err)
	}
	store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")
	RegisterTemp(encPath, encPath)
	t.Cleanup(func() { ForgetTemp(encPath) })
	syncErr := errors.New("directory sync failure")
	syncDirectoryFn = func(string) error { return syncErr }
	removed := false
	removeWALSiblingsFn = func(string) error {
		removed = true
		return nil
	}
	if err := store.CloseAt(encPath); !errors.Is(err, syncErr) {
		t.Fatalf("CloseAt error = %v, want %v", err, syncErr)
	}
	if removed {
		t.Fatal("WAL siblings were removed after directory sync failure")
	}
	regValue, ok := encryptedTemps.Load(encPath)
	if !ok {
		t.Fatal("directory sync failure discarded recovery registration")
	}
	reg, ok := tempRegistration(regValue)
	if !ok || reg.tmpPath != encPath {
		t.Fatalf("recovery registration = %+v, want canonical source", reg)
	}
	var recoveryPath string
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".symeraseme_recovery_") {
			recoveryPath = filepath.Join(dir, entry.Name())
			break
		}
	}
	if recoveryPath == "" {
		t.Fatal("directory sync failure discarded encrypted recovery copy")
	}
	recovered, err := os.ReadFile(recoveryPath)
	if err != nil {
		t.Fatalf("read recovery copy: %v", err)
	}
	if !bytes.HasPrefix(recovered, EncMagicV3) {
		t.Fatalf("recovery copy is not encrypted replacement: %q", recovered[:min(len(recovered), len(EncMagicV3))])
	}
	plainReplacement, err := DecryptBytes(recovered)
	if err != nil || !bytes.Equal(plainReplacement, plain) {
		t.Fatalf("recovery copy does not preserve replacement: decrypt=%v", err)
	}
	canonical, err := os.ReadFile(encPath)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(canonical, plain) {
		t.Fatalf("canonical source = %q, want prior plaintext %q", canonical, plain)
	}
}
