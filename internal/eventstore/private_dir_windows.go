//go:build windows

package eventstore

import "os"

// Windows mode bits do not enforce ACLs. Decrypted files therefore live under
// the user-scoped UserCacheDir selected by config.DefaultEncryptedTempDir and
// inherit the profile ACL; this helper deliberately does not claim POSIX
// 0700/0600 enforcement.
func ensurePrivateDir(path string) error {
	return os.MkdirAll(path, 0o700)
}

func ensurePrivateFile(string, os.FileMode) error { return nil }
