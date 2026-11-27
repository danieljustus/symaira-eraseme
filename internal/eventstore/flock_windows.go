//go:build windows

package eventstore

import (
	"fmt"
	"os"

	"golang.org/x/sys/windows"
)

// flockExclusive acquires an exclusive non-blocking Windows file lock.
func flockExclusive(f *os.File) error {
	if f == nil {
		return fmt.Errorf("eventstore: nil file")
	}
	overlapped := &windows.Overlapped{}
	return windows.LockFileEx(
		windows.Handle(f.Fd()),
		windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY,
		0,
		1,
		0,
		overlapped,
	)
}

// funlock releases a Windows file lock. Unlock errors are intentionally
// ignored during cleanup, matching the Unix implementation.
func funlock(f *os.File) {
	if f == nil {
		return
	}
	_ = windows.UnlockFileEx(windows.Handle(f.Fd()), 0, 1, 0, &windows.Overlapped{})
}
