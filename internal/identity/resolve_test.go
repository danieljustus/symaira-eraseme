package identity

import (
	"context"
	"errors"
	"os"
	"strings"
	"testing"
)

func TestResolveSecretPreservesLiteralValues(t *testing.T) {
	called := false
	original := resolveSharedSecret
	resolveSharedSecret = func(context.Context, string, string) (string, error) {
		called = true
		return "", errors.New("must not be called")
	}
	t.Cleanup(func() { resolveSharedSecret = original })

	got, err := ResolveSecret("already-resolved-secret", SecretResolver{})
	if err != nil {
		t.Fatalf("ResolveSecret() error = %v", err)
	}
	if got != "already-resolved-secret" {
		t.Fatalf("ResolveSecret() = %q, want literal value", got)
	}
	if called {
		t.Fatal("literal value must not be delegated to secretref")
	}
}

func TestResolveSecretDelegatesSupportedReferences(t *testing.T) {
	original := resolveSharedSecret
	t.Cleanup(func() { resolveSharedSecret = original })

	var calls []string
	resolveSharedSecret = func(_ context.Context, reference, envDefault string) (string, error) {
		calls = append(calls, reference+"|"+envDefault)
		return "resolved-secret", nil
	}

	cases := []struct {
		name     string
		value    string
		expected string
	}{
		{name: "environment", value: "env://SMTP_PASSWORD", expected: "env://SMTP_PASSWORD|"},
		{name: "keychain", value: "keychain://mail/account", expected: "keychain://mail/account|"},
		{name: "canonical vault", value: "symvault://team/mail", expected: "symvault://team/mail|"},
		{name: "legacy vault alias", value: "vault://team/mail", expected: "symvault://team/mail|"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			calls = nil
			got, err := ResolveSecret(tc.value, SecretResolver{})
			if err != nil {
				t.Fatalf("ResolveSecret() error = %v", err)
			}
			if got != "resolved-secret" {
				t.Fatalf("ResolveSecret() = %q, want shared result", got)
			}
			if len(calls) != 1 || calls[0] != tc.expected {
				t.Fatalf("secretref calls = %#v, want [%q]", calls, tc.expected)
			}
		})
	}
}

func TestResolveSecretUsesEnvironmentFallbackAfterVaultFailure(t *testing.T) {
	originalResolve := resolveSharedSecret
	originalKeyring := keyringGet
	t.Cleanup(func() {
		resolveSharedSecret = originalResolve
		keyringGet = originalKeyring
	})

	resolveSharedSecret = func(context.Context, string, string) (string, error) {
		return "", errors.New("vault unavailable")
	}
	keyringGet = func(string, string) (string, error) {
		t.Fatal("keyring must not be queried after an environment fallback succeeds")
		return "", nil
	}
	t.Setenv("SYMERASEME_TEST_SECRET", "env-secret")

	got, err := ResolveSecret("symvault://team/mail", SecretResolver{
		EnvFallback:    "SYMERASEME_TEST_SECRET",
		KeyringService: "symeraseme-test",
	})
	if err != nil {
		t.Fatalf("ResolveSecret() error = %v", err)
	}
	if got != "env-secret" {
		t.Fatalf("ResolveSecret() = %q, want environment fallback", got)
	}
}

func TestResolveSecretUsesKeyringFallbackWithVaultPathDefault(t *testing.T) {
	originalResolve := resolveSharedSecret
	originalKeyring := keyringGet
	t.Cleanup(func() {
		resolveSharedSecret = originalResolve
		keyringGet = originalKeyring
	})

	resolveSharedSecret = func(context.Context, string, string) (string, error) {
		return "", errors.New("vault unavailable")
	}
	keyringGet = func(service, username string) (string, error) {
		if service != "symeraseme-test" {
			t.Errorf("keyring service = %q, want symeraseme-test", service)
		}
		if username != "team/mail" {
			t.Errorf("keyring username = %q, want team/mail", username)
		}
		return "keyring-secret", nil
	}

	got, err := ResolveSecret("vault://team/mail", SecretResolver{
		KeyringService: "symeraseme-test",
	})
	if err != nil {
		t.Fatalf("ResolveSecret() error = %v", err)
	}
	if got != "keyring-secret" {
		t.Fatalf("ResolveSecret() = %q, want keyring fallback", got)
	}
}

func TestResolveSecretDoesNotReturnReferenceFallbackValues(t *testing.T) {
	originalResolve := resolveSharedSecret
	originalKeyring := keyringGet
	t.Cleanup(func() {
		resolveSharedSecret = originalResolve
		keyringGet = originalKeyring
	})

	resolveSharedSecret = func(context.Context, string, string) (string, error) {
		return "", errors.New("vault unavailable")
	}
	keyringGet = func(string, string) (string, error) {
		return "", errors.New("missing")
	}
	t.Setenv("SYMERASEME_TEST_SECRET", "env://OTHER_SECRET")

	_, err := ResolveSecret("symvault://team/mail", SecretResolver{
		EnvFallback:    "SYMERASEME_TEST_SECRET",
		KeyringService: "symeraseme-test",
	})
	if err == nil {
		t.Fatal("ResolveSecret() must fail when fallback contains another reference")
	}
	if strings.Contains(err.Error(), "keyring-secret") || strings.Contains(err.Error(), os.Getenv("SYMERASEME_TEST_SECRET")) {
		t.Fatalf("error leaked a fallback value: %v", err)
	}
}

func TestResolveSecretRejectsEmptyVaultReference(t *testing.T) {
	originalResolve := resolveSharedSecret
	originalKeyring := keyringGet
	t.Cleanup(func() {
		resolveSharedSecret = originalResolve
		keyringGet = originalKeyring
	})

	resolveSharedSecret = func(context.Context, string, string) (string, error) {
		t.Fatal("empty vault references must be rejected before delegation")
		return "", nil
	}
	keyringGet = func(string, string) (string, error) {
		t.Fatal("empty vault references must not reach the fallback chain")
		return "", nil
	}

	_, err := ResolveSecret("vault://", SecretResolver{KeyringService: "symeraseme-test"})
	if err == nil || !strings.Contains(err.Error(), "empty vault:// URI") {
		t.Fatalf("ResolveSecret() error = %v, want empty URI error", err)
	}
}
