//go:build !windows

package eventstore

import "os"

func ensurePrivateDir(path string) error {
	if err := os.MkdirAll(path, 0o700); err != nil {
		return err
	}
	return os.Chmod(path, 0o700)
}

func ensurePrivateFile(path string, mode os.FileMode) error {
	return os.Chmod(path, mode)
}
