//go:build !windows

package eventstore

import "os"

// replaceFile atomically replaces dst with src. Both paths must be in the
// same filesystem; atomicWriteFile creates src next to dst to guarantee that.
func replaceFile(src, dst string) error {
	return os.Rename(src, dst)
}
