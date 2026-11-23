//go:build windows

package eventstore

import (
	"golang.org/x/sys/windows"
)

// replaceFile atomically replaces dst with src on Windows. MoveFileEx with
// REPLACE_EXISTING is the Windows equivalent of same-directory rename over an
// existing file; WRITE_THROUGH asks the OS to flush the move before returning.
func replaceFile(src, dst string) error {
	srcPath, err := windows.UTF16PtrFromString(src)
	if err != nil {
		return err
	}
	dstPath, err := windows.UTF16PtrFromString(dst)
	if err != nil {
		return err
	}
	return windows.MoveFileEx(srcPath, dstPath, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH)
}
