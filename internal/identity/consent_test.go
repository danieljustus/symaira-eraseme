package identity

import (
	"context"
	"errors"
	"testing"
)

// TestConsentTokenLifecycle: issue → verify → consume → revoked.
func TestConsentTokenLifecycle(t *testing.T) {
	dir := t.TempDir()
	SetNowFunc(func() int64 { return 1_000_000 })

	token, err := IssueTokenInDir(dir, "send-removal", 3600)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if token == "" {
		t.Fatal("empty token")
	}

	if err := VerifyTokenInDir(dir, "send-removal", token); err != nil {
		t.Errorf("verify fresh token: %v", err)
	}
	// Wrong command must fail.
	if err := VerifyTokenInDir(dir, "other-command", token); err == nil {
		t.Error("token for different command must not verify")
	}
	// Expired token must fail.
	SetNowFunc(func() int64 { return 1_000_000 + 3601 })
	if err := VerifyTokenInDir(dir, "send-removal", token); err == nil {
		t.Error("expired token must not verify")
	}
	SetNowFunc(func() int64 { return 1_000_000 })

	// Consume removes the token.
	if err := ConsumeTokenInDir(dir, token); err != nil {
		t.Fatalf("consume: %v", err)
	}
	if err := VerifyTokenInDir(dir, "send-removal", token); err == nil {
		t.Error("consumed token must not verify")
	}

	SetNowFunc(nil)
}

// TestConsentListAndRevoke.
func TestConsentListAndRevoke(t *testing.T) {
	dir := t.TempDir()
	SetNowFunc(func() int64 { return 1_000_000 })
	defer SetNowFunc(nil)

	tok1, _ := IssueTokenInDir(dir, "cmd-a", 3600)
	tok2, _ := IssueTokenInDir(dir, "cmd-b", 3600)

	tokens, err := ListTokensInDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(tokens) != 2 {
		t.Fatalf("expected 2 tokens, got %d", len(tokens))
	}

	ok, err := RevokeTokenInDir(dir, tok1)
	if err != nil || !ok {
		t.Fatalf("revoke: %v ok=%v", err, ok)
	}
	if err := VerifyTokenInDir(dir, "cmd-a", tok1); err == nil {
		t.Error("revoked token must not verify")
	}
	_ = tok2
}

// TestSecretResolverURISplit: plain values pass through; vault URIs route
// into the symvault layer, where test seams decide the outcome.
func TestSecretResolverURISplit(t *testing.T) {
	// Plain value passes through untouched.
	got, err := ResolveSecret("plain-value", SecretResolver{})
	if err != nil || got != "plain-value" {
		t.Errorf("plain value: got %q err %v", got, err)
	}

	// vault:// and symvault:// are both recognized as vault URIs.
	for _, uri := range []string{"vault://x/y", "symvault://x/y"} {
		called := false
		opts := SecretResolver{
			RunSymvault: func(ctx context.Context, path, vaultPath string) (string, error) {
				called = true
				return "from-vault", nil
			},
			SymvaultPath: "/fake/symvault",
		}
		got, err := ResolveSecret(uri, opts)
		if err != nil {
			t.Errorf("%s: %v", uri, err)
			continue
		}
		if !called {
			t.Errorf("%s: symvault layer not invoked", uri)
		}
		if got != "from-vault" {
			t.Errorf("%s: got %q", uri, got)
		}
	}

	// A failing symvault call surfaces the error (no silent fallback to
	// the raw URI — that would leak the vault path into output).
	opts := SecretResolver{
		RunSymvault: func(ctx context.Context, path, vaultPath string) (string, error) {
			return "", errors.New("vault unavailable")
		},
		SymvaultPath: "/fake/symvault",
	}
	if _, err := ResolveSecret("symvault://secret/entry", opts); err == nil {
		t.Error("failing vault lookup must error")
	}
}
