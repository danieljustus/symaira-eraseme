// Package identity — bootstrap.go: glue that ties the identity
// master key to the eventstore encryption layer.
//
// The Go event store (internal/eventstore) reads its 32-byte master
// key through a MasterKeyProvider.  Bootstrap installs an identity
// MasterKeyProvider that resolves the key through the same lookup
// chain GetExistingMasterKey uses (env var, keyring, fresh generate
// for first-time setup), so callers do not have to manage the
// eventstore's package-level key separately.
package identity

import (
	"fmt"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

// Bootstrap wires the identity master key as the source of the
// eventstore encryption key.  After Bootstrap returns successfully,
// the eventstore will read its key through GetExistingMasterKey (or
// fall through to keyring / env vars) on every decrypt call.
//
// Returns the master key that was wired in so callers can echo it
// in startup logs without revealing the value.
func Bootstrap() ([]byte, error) {
	key, err := GetOrCreateMasterKey()
	if err != nil {
		return nil, fmt.Errorf("identity: bootstrap master key: %w", err)
	}
	eventstore.SetMasterKeyProvider(func() ([]byte, error) {
		// Re-resolve each call so a key rotation or env-var
		// flip after process start is picked up by the store.
		return GetExistingMasterKey()
	})
	return key, nil
}

// BootstrapReadOnly wires the master key provider but refuses to
// create a new key when none is present.  Use this in read-only
// commands (e.g. `symeraseme status`) so a missing key surfaces as
// ErrMasterKeyMissing instead of silently minting a new identity.
func BootstrapReadOnly() error {
	if _, err := GetExistingMasterKey(); err != nil {
		return err
	}
	eventstore.SetMasterKeyProvider(func() ([]byte, error) {
		return GetExistingMasterKey()
	})
	return nil
}

// Shutdown clears the in-process master-key cache and removes the
// eventstore provider.  Mirrors a clean logout.
func Shutdown() {
	_ = SetMasterKey(nil)
	eventstore.SetMasterKeyProvider(nil)
}
