package registry

import (
	"io/fs"
	"os"
)

// openedRootFS keeps the filesystem view used for walking paired with the
// opened root handle used for race-safe file operations.
type openedRootFS struct {
	fs.FS
	root *os.Root
}

func (r openedRootFS) Lstat(name string) (os.FileInfo, error) {
	return r.root.Lstat(name)
}

func (r openedRootFS) OpenFile(name string, flag int, perm os.FileMode) (*os.File, error) {
	return r.root.OpenFile(name, flag, perm)
}
