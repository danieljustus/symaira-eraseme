// Package identity — resolve.go: symvault:// and vault:// secret
// resolution.  Mirrors src/symeraseme/core/secrets.py in the
// Go port.
//
// Resolution order (highest priority first):
//
//  1. Literal value (no prefix → returned as-is).
//  2. “symvault get <path> --print“ subprocess (5s timeout).
//  3. “env_fallback“ env var (only when symvault failed).
//  4. Keyring lookup (only when env_fallback also failed).
//
// The shorter “vault://“ form is accepted as a deprecated alias
// and resolves to the same paths — see the AGENTS.md / README
// section on the symvault:// vs vault:// URI split.
package identity

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"

	"time"

	"github.com/zalando/go-keyring"
)

// Vault URI prefixes.  The order matters: the longer symvault:// is
// checked first so that the legacy "vault://" prefix never shadows
// the canonical one.
var (
	VaultPrefixSymvault = "symvault://"
	VaultPrefixVault    = "vault://"
	vaultPrefixes       = []string{VaultPrefixSymvault, VaultPrefixVault}
)

// SymvaultTimeout is the subprocess timeout for “symvault get“.
// Mirrors _SYMVAULT_TIMEOUT = 5.
const SymvaultTimeout = 5 * time.Second

// ErrSecretResolution is returned when no layer can supply a
// secret.  Wraps the underlying reason in the message but never
// the secret value itself.
var ErrSecretResolution = errors.New("identity: cannot resolve secret")

// SecretResolver is the per-process configuration for ResolveSecret.
// A zero value uses the defaults (symvault on PATH, 5s timeout).
type SecretResolver struct {
	// EnvFallback, when non-empty, names the environment variable
	// to consult if the symvault layer returns nothing.
	EnvFallback string
	// KeyringService, when non-empty, names the keyring service
	// to consult as a last resort.
	KeyringService string
	// KeyringUsername, when non-empty, names the keyring account.
	// Defaults to the value (sans prefix) of the vault URI.
	KeyringUsername string
	// Timeout overrides the default subprocess timeout.
	Timeout time.Duration
	// SymvaultPath overrides the symvault binary lookup.  When
	// empty, exec.LookPath("symvault") is used.
	SymvaultPath string
	// LookPath is an indirection for executable lookup (tests).
	LookPath func(string) (string, error)
	// RunSymvault is the actual subprocess invocation.  Tests
	// override this to skip the real CLI.
	RunSymvault func(ctx context.Context, path, vaultPath string) (string, error)
}

// ResolveSecret evaluates the fallback chain.  When value is not
// a vault URI it is returned as-is.  The error never includes the
// secret value (only the env var name, which is not a secret).
func ResolveSecret(value string, opts SecretResolver) (string, error) {
	prefix := vaultPrefix(value)
	if prefix == "" {
		return value, nil
	}
	vaultPath := value[len(prefix):]
	if vaultPath == "" {
		return "", fmt.Errorf("%w: empty %s URI (provide a path like %spart/key)",
			ErrSecretResolution, prefix, VaultPrefixSymvault)
	}

	// 1. symvault subprocess.
	if secret, err := callSymvault(opts, vaultPath); err == nil && secret != "" {
		return secret, nil
	}

	// 2. env fallback.
	if opts.EnvFallback != "" {
		if v := os.Getenv(opts.EnvFallback); v != "" {
			return v, nil
		}
	}

	// 3. keyring.
	if opts.KeyringService != "" {
		username := opts.KeyringUsername
		if username == "" {
			username = vaultPath
		}
		if v, err := keyring.Get(opts.KeyringService, username); err == nil && v != "" {
			return v, nil
		}
	}

	// All layers exhausted.
	msg := "symvault not available or returned error"
	if opts.EnvFallback != "" {
		msg += fmt.Sprintf(", env var %q not set", opts.EnvFallback)
	}
	if opts.KeyringService != "" {
		msg += fmt.Sprintf(", keyring %q has no entry", opts.KeyringService)
	}
	msg += ". Set the value directly or install symvault."
	return "", fmt.Errorf("%w: %s", ErrSecretResolution, msg)
}

// vaultPrefix returns the matched vault prefix (or "" for none).
func vaultPrefix(v string) string {
	for _, p := range vaultPrefixes {
		if strings.HasPrefix(v, p) {
			return p
		}
	}
	return ""
}

// callSymvault invokes the symvault CLI.  Tests override
// opts.RunSymvault to return canned output without spawning a
// process.
func callSymvault(opts SecretResolver, vaultPath string) (string, error) {
	if opts.RunSymvault != nil {
		ctx, cancel := context.WithTimeout(context.Background(), symvaultTimeout(opts))
		defer cancel()
		return opts.RunSymvault(ctx, opts.SymvaultPath, vaultPath)
	}
	bin, err := symvaultLookPath(opts)
	if err != nil {
		return "", err
	}
	ctx, cancel := context.WithTimeout(context.Background(), symvaultTimeout(opts))
	defer cancel()
	cmd := exec.CommandContext(ctx, bin, "get", vaultPath, "--print") //nolint:gosec // resolved path
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return "", err
	}
	if ctx.Err() == context.DeadlineExceeded {
		return "", context.DeadlineExceeded
	}
	secret := strings.TrimRight(stdout.String(), "\r\n")
	if secret == "" {
		return "", errors.New("symvault returned empty output")
	}
	return secret, nil
}

func symvaultLookPath(opts SecretResolver) (string, error) {
	if opts.SymvaultPath != "" {
		return opts.SymvaultPath, nil
	}
	look := opts.LookPath
	if look == nil {
		look = exec.LookPath
	}
	return look("symvault")
}

func symvaultTimeout(opts SecretResolver) time.Duration {
	if opts.Timeout > 0 {
		return opts.Timeout
	}
	return SymvaultTimeout
}
