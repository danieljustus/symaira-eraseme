//go:build windows

package eventstore

// Windows has no portable syncable directory handle. replaceFile uses
// MoveFileEx with MOVEFILE_WRITE_THROUGH, which provides the file replacement
// durability available for this platform.
func syncDirectory(string) error { return nil }
