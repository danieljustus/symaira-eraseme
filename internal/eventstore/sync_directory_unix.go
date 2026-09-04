//go:build !windows

package eventstore

import "os"

// syncDirectory persists the directory entry created by a replacement rename.
func syncDirectory(path string) error {
	f, err := os.Open(path) //nolint:gosec // directory derived from caller path
	if err != nil {
		return err
	}
	defer f.Close()
	return f.Sync()
}
