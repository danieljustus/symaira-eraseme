// Package eventstore (flock_unix.go): POSIX advisory lock helpers.
// Builds on macOS and Linux; on Windows this file is replaced by
// flock_windows.go which falls back to a no-op (the file-exists
// check is the best we can do without platform support).
//go:build !windows

package eventstore

import (
	"fmt"
	"os"

	"golang.org/x/sys/unix"
)

// flockExclusive acquires an exclusive non-blocking flock on f.
// Returns an error when the lock is already held by another
// process.
func flockExclusive(f *os.File) error {
	if f == nil {
		return fmt.Errorf("eventstore: nil file")
	}
	return unix.Flock(int(f.Fd()), unix.LOCK_EX|unix.LOCK_NB)
}

// funlock releases the flock on f.  Safe to call on a file that
// never acquired a lock — unix.Flock returns EINVAL in that case
// and we ignore it.
func funlock(f *os.File) {
	if f == nil {
		return
	}
	_ = unix.Flock(int(f.Fd()), unix.LOCK_UN)
}
