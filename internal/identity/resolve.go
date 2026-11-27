// Package identity — resolve.go: shared secret-reference resolution with
// an EraseMe-specific keyring fallback.  The shared corekit package owns
// prefix parsing and hardened symvault/keychain subprocess handling.
//
// Resolution order (highest priority first):
//
//  1. Literal value (no supported URI prefix → returned as-is).
//  2. Shared corekit secretref resolution for env://, keychain://,
//     symvault://, and the legacy vault:// alias.
//  3. “env_fallback“ env var (only when vault resolution fails).
//  4. Keyring lookup (only when env_fallback also failed).
//
// The shorter “vault://“ form is accepted as a deprecated alias and is
// normalized to the canonical symvault:// reference before delegation.
package identity

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/danieljustus/symaira-corekit/secretref"
)

// Vault URI prefixes. The order matters: the longer symvault:// is checked
// first so that the legacy vault:// prefix never shadows the canonical one.
const (
	VaultPrefixSymvault = "symvault://"
	VaultPrefixVault    = "vault://"
	envPrefix           = "env://"
	keychainPrefix      = "keychain://"
)

// ErrSecretResolution is returned when no resolution layer can supply a
// secret. The message never includes a resolved secret value.
var ErrSecretResolution = errors.New("identity: cannot resolve secret")

// resolveSharedSecret is indirected for tests. Production calls always use
// the shared corekit resolver; the indirection keeps tests independent of
// installed binaries and OS keychain contents.
var resolveSharedSecret = secretref.Resolve

// keyringGet is indirected for tests so the EraseMe-specific final fallback
// can be verified without reading the user's real keyring.
var keyringGet = func(service, username string) (string, error) {
	return getKeyring().Get(service, username)
}

// SecretResolver is the EraseMe-specific configuration for ResolveSecret.
// Shared URI parsing and subprocess timeouts are owned by corekit/secretref.
type SecretResolver struct {
	// EnvFallback, when non-empty, names the environment variable to consult
	// if a symvault reference cannot be resolved.
	EnvFallback string
	// KeyringService, when non-empty, names the keyring service to consult as
	// a last resort.
	KeyringService string
	// KeyringUsername, when non-empty, names the keyring account. It defaults
	// to the value (sans URI prefix) of the vault reference.
	KeyringUsername string
}

// ResolveSecret evaluates the shared secret-reference resolver and the
// EraseMe-specific fallback chain. Non-reference values are deliberately
// returned unchanged because existing callers pass already-resolved literal
// credentials here.
func ResolveSecret(value string, opts SecretResolver) (string, error) {
	switch {
	case strings.HasPrefix(value, envPrefix), strings.HasPrefix(value, keychainPrefix):
		secret, err := resolveSharedSecret(context.Background(), value, "")
		if err != nil {
			return "", fmt.Errorf("%w: %v", ErrSecretResolution, err)
		}
		return secret, nil
	case strings.HasPrefix(value, VaultPrefixSymvault), strings.HasPrefix(value, VaultPrefixVault):
		return resolveVaultSecret(value, opts)
	default:
		return value, nil
	}
}

func resolveVaultSecret(value string, opts SecretResolver) (string, error) {
	prefix := vaultPrefix(value)
	vaultPath := value[len(prefix):]
	if vaultPath == "" {
		return "", fmt.Errorf("%w: empty %s URI (provide a path like %spart/key)",
			ErrSecretResolution, prefix, VaultPrefixSymvault)
	}

	reference := value
	if prefix == VaultPrefixVault {
		reference = VaultPrefixSymvault + vaultPath
	}

	// Corekit owns prefix parsing, path validation, the `--` separator, and
	// the default subprocess timeout. Do not duplicate that implementation.
	if secret, err := resolveSharedSecret(context.Background(), reference, ""); err == nil && secret != "" {
		return secret, nil
	}

	// EraseMe-specific fallback chain for vault references.
	if opts.EnvFallback != "" {
		if value := os.Getenv(opts.EnvFallback); value != "" && !isSecretReference(value) {
			return value, nil
		}
	}

	if opts.KeyringService != "" {
		username := opts.KeyringUsername
		if username == "" {
			username = vaultPath
		}
		if value, err := keyringGet(opts.KeyringService, username); err == nil && value != "" {
			return value, nil
		}
	}

	msg := "shared secret reference could not be resolved"
	if opts.EnvFallback != "" {
		msg += fmt.Sprintf(", env var %q not set", opts.EnvFallback)
	}
	if opts.KeyringService != "" {
		msg += fmt.Sprintf(", keyring %q has no entry", opts.KeyringService)
	}
	msg += ". Set the value directly or configure a supported secret reference."
	return "", fmt.Errorf("%w: %s", ErrSecretResolution, msg)
}

// vaultPrefix returns the matched vault prefix (or "" for none).
func vaultPrefix(value string) string {
	if strings.HasPrefix(value, VaultPrefixSymvault) {
		return VaultPrefixSymvault
	}
	if strings.HasPrefix(value, VaultPrefixVault) {
		return VaultPrefixVault
	}
	return ""
}

func isSecretReference(value string) bool {
	return strings.HasPrefix(value, VaultPrefixSymvault) ||
		strings.HasPrefix(value, VaultPrefixVault) ||
		strings.HasPrefix(value, envPrefix) ||
		strings.HasPrefix(value, keychainPrefix)
}
