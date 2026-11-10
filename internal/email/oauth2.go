package email

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

var (
	ErrOAuth2      = errors.New("email: oauth2 error")
	ErrOAuth2State = errors.New("email: oauth2 state error")
)

// ProviderConfig describes the provider endpoints and scopes used for email.
type ProviderConfig struct {
	AuthURL  string
	TokenURL string
	Scopes   string
}

var ProviderConfigs = map[string]ProviderConfig{
	"gmail": {
		AuthURL:  "https://accounts.google.com/o/oauth2/v2/auth",
		TokenURL: "https://oauth2.googleapis.com/token",
		Scopes:   "https://mail.google.com/ https://www.googleapis.com/auth/gmail.send",
	},
	"outlook": {
		AuthURL:  "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
		TokenURL: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
		Scopes:   "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access",
	},
}

// OAuthStateStore persists one-time authorization states with restrictive file
// permissions and atomic replacement. It is intentionally independent of the
// HTTP client so tests never need a browser or provider account.
type OAuthStateStore struct {
	Path string
	TTL  time.Duration
	Now  func() time.Time
	mu   sync.Mutex
}

func NewOAuthStateStore(path string) *OAuthStateStore {
	return &OAuthStateStore{Path: path, TTL: 5 * time.Minute, Now: time.Now}
}

func (s *OAuthStateStore) Store(state, provider string) error {
	if state == "" || provider == "" {
		return fmt.Errorf("%w: state and provider are required", ErrOAuth2State)
	}
	return s.update(func(records map[string]oauthState) error {
		records[state] = oauthState{Provider: provider, ExpiresAt: s.now().Add(s.ttl()).Unix()}
		return nil
	})
}

// Validate consumes state only after it matches and has not expired.
func (s *OAuthStateStore) Validate(state string) error {
	if state == "" {
		return fmt.Errorf("%w: missing state (possible CSRF attack)", ErrOAuth2State)
	}
	return s.update(func(records map[string]oauthState) error {
		record, ok := records[state]
		if !ok {
			return fmt.Errorf("%w: state mismatch (possible CSRF attack)", ErrOAuth2State)
		}
		delete(records, state)
		if record.ExpiresAt < s.now().Unix() {
			return fmt.Errorf("%w: state expired (possible CSRF attack)", ErrOAuth2State)
		}
		return nil
	})
}

type oauthState struct {
	Provider  string `json:"provider"`
	ExpiresAt int64  `json:"expires_at"`
}

func (s *OAuthStateStore) now() time.Time {
	if s.Now != nil {
		return s.Now()
	}
	return time.Now()
}

func (s *OAuthStateStore) ttl() time.Duration {
	if s.TTL > 0 {
		return s.TTL
	}
	return 5 * time.Minute
}

func (s *OAuthStateStore) update(fn func(map[string]oauthState) error) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.Path == "" {
		return fmt.Errorf("%w: state path is empty", ErrOAuth2State)
	}
	if err := os.MkdirAll(filepath.Dir(s.Path), 0o700); err != nil {
		return fmt.Errorf("%w: state directory: %v", ErrOAuth2State, err)
	}
	records := map[string]oauthState{}
	if raw, err := os.ReadFile(s.Path); err == nil && len(raw) != 0 {
		if err := json.Unmarshal(raw, &records); err != nil {
			return fmt.Errorf("%w: invalid state file", ErrOAuth2State)
		}
	} else if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("%w: read state file: %v", ErrOAuth2State, err)
	}
	if err := fn(records); err != nil {
		return err
	}
	raw, err := json.Marshal(records)
	if err != nil {
		return fmt.Errorf("%w: encode state file: %v", ErrOAuth2State, err)
	}
	tmp, err := os.CreateTemp(filepath.Dir(s.Path), ".oauth2-state-*")
	if err != nil {
		return fmt.Errorf("%w: create state file: %v", ErrOAuth2State, err)
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("%w: state permissions: %v", ErrOAuth2State, err)
	}
	if _, err := tmp.Write(raw); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("%w: write state file: %v", ErrOAuth2State, err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("%w: close state file: %v", ErrOAuth2State, err)
	}
	if err := os.Chmod(s.Path, 0o600); err == nil {
		// Keep the existing destination restrictive before replacement where it exists.
		_ = err
	}
	if err := os.Rename(tmpName, s.Path); err != nil {
		return fmt.Errorf("%w: commit state file: %v", ErrOAuth2State, err)
	}
	return nil
}

// OAuth2Client implements provider URL construction and token exchange. HTTP
// responses are intentionally reduced to status-only errors to avoid leaking
// provider error bodies that may contain credentials or authorization codes.
type OAuth2Client struct {
	HTTPClient *http.Client
	States     *OAuthStateStore
	Now        func() time.Time
}

func (c *OAuth2Client) client() *http.Client {
	if c.HTTPClient != nil {
		return c.HTTPClient
	}
	return &http.Client{Timeout: 30 * time.Second}
}

func (c *OAuth2Client) stateStore() *OAuthStateStore {
	if c.States != nil {
		return c.States
	}
	return NewOAuthStateStore(defaultOAuthStatePath())
}

// AuthorizeURL returns an OAuth2 authorization URL and a PKCE verifier.
func (c *OAuth2Client) AuthorizeURL(provider, clientID, redirectURI string) (string, string, error) {
	cfg, ok := ProviderConfigs[strings.ToLower(strings.TrimSpace(provider))]
	if !ok {
		return "", "", fmt.Errorf("%w: unknown provider %q", ErrOAuth2, provider)
	}
	verifierBytes := make([]byte, 64)
	if _, err := rand.Read(verifierBytes); err != nil {
		return "", "", fmt.Errorf("%w: generate PKCE verifier: %v", ErrOAuth2, err)
	}
	verifier := base64.RawURLEncoding.EncodeToString(verifierBytes)
	hash := sha256.Sum256([]byte(verifier))
	challenge := base64.RawURLEncoding.EncodeToString(hash[:])
	stateBytes := make([]byte, 16)
	if _, err := rand.Read(stateBytes); err != nil {
		return "", "", fmt.Errorf("%w: generate state: %v", ErrOAuth2, err)
	}
	state := base64.RawURLEncoding.EncodeToString(stateBytes)
	if err := c.stateStore().Store(state, strings.ToLower(strings.TrimSpace(provider))); err != nil {
		return "", "", err
	}
	params := url.Values{
		"client_id":             {clientID},
		"redirect_uri":          {redirectURI},
		"response_type":         {"code"},
		"scope":                 {cfg.Scopes},
		"access_type":           {"offline"},
		"prompt":                {"consent"},
		"state":                 {state},
		"code_challenge":        {challenge},
		"code_challenge_method": {"S256"},
	}
	return cfg.AuthURL + "?" + params.Encode(), verifier, nil
}

// ExchangeCode exchanges an authorization code without logging it or the
// client secret. The returned map is provider JSON and should be stored in a
// secure credential boundary by the caller.
func (c *OAuth2Client) ExchangeCode(ctx context.Context, provider, code, clientID, clientSecret, redirectURI, verifier string) (map[string]any, error) {
	cfg, ok := ProviderConfigs[strings.ToLower(strings.TrimSpace(provider))]
	if !ok {
		return nil, fmt.Errorf("%w: unknown provider %q", ErrOAuth2, provider)
	}
	form := url.Values{"code": {code}, "client_id": {clientID}, "client_secret": {clientSecret}, "redirect_uri": {redirectURI}, "grant_type": {"authorization_code"}}
	if verifier != "" {
		form.Set("code_verifier", verifier)
	}
	return c.tokenRequest(ctx, cfg.TokenURL, form, "token exchange")
}

func (c *OAuth2Client) RefreshAccessToken(ctx context.Context, provider, clientID, clientSecret, refreshToken string) (map[string]any, error) {
	cfg, ok := ProviderConfigs[strings.ToLower(strings.TrimSpace(provider))]
	if !ok {
		return nil, fmt.Errorf("%w: unknown provider %q", ErrOAuth2, provider)
	}
	form := url.Values{"refresh_token": {refreshToken}, "client_id": {clientID}, "client_secret": {clientSecret}, "grant_type": {"refresh_token"}}
	return c.tokenRequest(ctx, cfg.TokenURL, form, "token refresh")
}

func (c *OAuth2Client) tokenRequest(ctx context.Context, endpoint string, form url.Values, operation string) (map[string]any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, fmt.Errorf("%w: %s request: %v", ErrOAuth2, operation, err)
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	resp, err := c.client().Do(req)
	if err != nil {
		return nil, fmt.Errorf("%w: %s failed", ErrOAuth2, operation)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("%w: %s returned HTTP %d", ErrOAuth2, operation, resp.StatusCode)
	}
	var result map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("%w: %s returned invalid JSON", ErrOAuth2, operation)
	}
	return result, nil
}

func defaultOAuthStatePath() string {
	base := os.Getenv("XDG_DATA_HOME")
	if base == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return filepath.Join(".", ".symeraseme", "oauth2_state.json")
		}
		base = filepath.Join(home, ".local", "share")
	}
	return filepath.Join(base, "symeraseme", "oauth2_state.json")
}
