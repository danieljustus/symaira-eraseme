// Command oauth2 is the byte oracle for `internal/email/oauth2.go` (register row
// DOM-007: OAuth2 state, PKCE, refresh and redaction).
//
// Two kinds of value are recorded here:
//
//   - Deterministic behaviour: the provider table, the PKCE derivation, the
//     one-time state store (including expiry, consumption and file format) and
//     every error string.
//   - Request behaviour: what actually goes on the wire for a token exchange or
//     refresh. The provider endpoints are fixed upstream URLs, so the client's
//     HTTP transport is rewritten onto a local server — the same trick the
//     package's own tests use. The transcript (method, path, content type, form
//     body) is recorded and replayed against the Rust port.
//
// The random parts of an authorization URL (state, PKCE verifier) cannot be
// pinned by value; the case records their lengths, whether the challenge is the
// S256 derivation of the returned verifier, and the parameter map with those two
// values replaced by "<random>".
//
// Usage:
//
//	GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/oauth2            # stdout
//	GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/oauth2 --fixture  # writes the fixture
//
// The document is stable: running it twice must produce identical bytes.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
)

const (
	sourceRevision = "464e165ba61bb1c13ef95bd2e610747d9a67475f"
	sourcePath     = "internal/email/oauth2.go"
	fixturePath    = "rust-tests/parity/oracle/oauth2/oauth2_cases.json"
	randomMark     = "<random>"
)

type document struct {
	SourceRevision string                          `json:"source_revision"`
	SourcePath     string                          `json:"source_path"`
	Providers      map[string]email.ProviderConfig `json:"providers"`
	Authorize      []authorizeCase                 `json:"authorize"`
	Pkce           []pkceCase                      `json:"pkce"`
	StateStore     []stateCase                     `json:"state_store"`
	Tokens         []tokenCase                     `json:"tokens"`
}

type authorizeCase struct {
	Name            string              `json:"name"`
	Provider        string              `json:"provider"`
	ClientID        string              `json:"client_id"`
	RedirectURI     string              `json:"redirect_uri"`
	AuthURLPrefix   string              `json:"auth_url_prefix"`
	Params          map[string][]string `json:"params"`
	VerifierLength  int                 `json:"verifier_length"`
	StateLength     int                 `json:"state_length"`
	ChallengeIsS256 bool                `json:"challenge_is_s256_of_verifier"`
	Error           string              `json:"error,omitempty"`
}

type pkceCase struct {
	Verifier  string `json:"verifier"`
	Challenge string `json:"challenge"`
}

type stateStep struct {
	Op       string `json:"op"`
	State    string `json:"state"`
	Provider string `json:"provider,omitempty"`
	At       string `json:"at"`
	Error    string `json:"error,omitempty"`
}

type stateCase struct {
	Name       string      `json:"name"`
	PathSet    bool        `json:"path_set"`
	TTLSeconds int         `json:"ttl_seconds"`
	Steps      []stateStep `json:"steps"`
	File       string      `json:"file,omitempty"`
	FileMode   int         `json:"file_mode"`
}

type tokenCase struct {
	Name      string            `json:"name"`
	Operation string            `json:"operation"`
	Provider  string            `json:"provider"`
	Params    map[string]string `json:"params"`
	Response  struct {
		Status int    `json:"status"`
		Body   string `json:"body"`
	} `json:"response"`
	TransportFails bool `json:"transport_fails,omitempty"`
	Recorded       struct {
		Method      string `json:"method"`
		Path        string `json:"path"`
		ContentType string `json:"content_type"`
		Body        string `json:"body"`
	} `json:"recorded"`
	Result string `json:"result,omitempty"`
	Error  string `json:"error,omitempty"`
}

func main() {
	writeFixture := flag.Bool("fixture", false, "write the fixture instead of stdout")
	flag.Parse()

	doc := document{
		SourceRevision: sourceRevision,
		SourcePath:     sourcePath,
		Providers:      email.ProviderConfigs,
		Authorize:      authorizeCases(),
		Pkce:           pkceCases(),
		StateStore:     stateCases(),
		Tokens:         tokenCases(),
	}
	raw, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		fail("encode: %v", err)
	}
	raw = append(raw, '\n')
	if !*writeFixture {
		os.Stdout.Write(raw)
		return
	}
	path := fixturePath
	if err := os.WriteFile(path, raw, 0o644); err != nil {
		fail("write fixture: %v", err)
	}
	fmt.Fprintf(os.Stderr, "wrote %s\n", path)
}

func authorizeCases() []authorizeCase {
	specs := []struct {
		name     string
		provider string
		clientID string
		redirect string
	}{
		{"gmail", "gmail", "client-123.apps.googleusercontent.com", "https://erase.example/callback"},
		{"outlook", " Outlook ", "client-abc", "http://localhost:8765/callback"},
		{"unknown_provider", "posteo", "client-123", "https://erase.example/callback"},
	}
	var out []authorizeCase
	for _, spec := range specs {
		storePath := filepath.Join(mustTempDir(), "oauth2_state.json")
		client := &email.OAuth2Client{States: email.NewOAuthStateStore(storePath)}
		rawURL, verifier, err := client.AuthorizeURL(spec.provider, spec.clientID, spec.redirect)
		if err != nil {
			out = append(out, authorizeCase{
				Name: spec.name, Provider: spec.provider, ClientID: spec.clientID,
				RedirectURI: spec.redirect, Error: err.Error(),
			})
			continue
		}
		parsed, err := url.Parse(rawURL)
		if err != nil {
			fail("parse authorize url: %v", err)
		}
		params := parsed.Query()
		challenge := params.Get("code_challenge")
		sum := sha256.Sum256([]byte(verifier))
		expected := base64.RawURLEncoding.EncodeToString(sum[:])
		state := params.Get("state")
		params.Set("state", randomMark)
		params.Set("code_challenge", randomMark)
		out = append(out, authorizeCase{
			Name: spec.name, Provider: spec.provider, ClientID: spec.clientID,
			RedirectURI:     spec.redirect,
			AuthURLPrefix:   email.ProviderConfigs[strings.ToLower(strings.TrimSpace(spec.provider))].AuthURL + "?",
			Params:          map[string][]string(params),
			VerifierLength:  len(verifier),
			StateLength:     len(state),
			ChallengeIsS256: challenge == expected,
		})
	}
	return out
}

func pkceCases() []pkceCase {
	specs := []string{"", "a", "test-verifier", strings.Repeat("x", 86), "verifier-ä-ö", "3f9a2b"}
	out := make([]pkceCase, 0, len(specs))
	for _, verifier := range specs {
		sum := sha256.Sum256([]byte(verifier))
		out = append(out, pkceCase{
			Verifier:  verifier,
			Challenge: base64.RawURLEncoding.EncodeToString(sum[:]),
		})
	}
	return out
}

func stateCases() []stateCase {
	base := time.Date(2026, 9, 19, 12, 0, 0, 0, time.UTC)
	specs := []struct {
		name    string
		pathSet bool
		ttl     int
		steps   []struct {
			op       string
			state    string
			provider string
			offset   time.Duration
		}
	}{
		{
			name: "store_validate_and_consume", pathSet: true, ttl: 300,
			steps: []struct {
				op, state, provider string
				offset              time.Duration
			}{
				{"store", "state-1", "gmail", 0},
				{"store", "state-2", "outlook", 0},
				{"validate", "state-1", "", 30 * time.Second},
				{"validate", "state-1", "", 31 * time.Second},
				{"validate", "state-2", "", 32 * time.Second},
			},
		},
		{
			name: "expired_state_is_rejected_and_consumed", pathSet: true, ttl: 60,
			steps: []struct {
				op, state, provider string
				offset              time.Duration
			}{
				{"store", "state-1", "gmail", 0},
				{"validate", "state-1", "", 61 * time.Second},
				{"validate", "state-1", "", 62 * time.Second},
			},
		},
		{
			name: "missing_and_empty_state", pathSet: true, ttl: 300,
			steps: []struct {
				op, state, provider string
				offset              time.Duration
			}{
				{"validate", "", "", 0},
				{"validate", "never-stored", "", 0},
				{"store", "", "gmail", 0},
				{"store", "state-1", "", 0},
			},
		},
		{
			name: "empty_path_is_rejected", pathSet: false, ttl: 300,
			steps: []struct {
				op, state, provider string
				offset              time.Duration
			}{
				{"store", "state-1", "gmail", 0},
			},
		},
	}
	out := make([]stateCase, 0, len(specs))
	for _, spec := range specs {
		path := filepath.Join(mustTempDir(), "oauth2_state.json")
		if !spec.pathSet {
			path = ""
		}
		store := &email.OAuthStateStore{Path: path, TTL: time.Duration(spec.ttl) * time.Second}
		offset := time.Duration(0)
		store.Now = func() time.Time { return base.Add(offset) }
		caseOut := stateCase{Name: spec.name, PathSet: spec.pathSet, TTLSeconds: spec.ttl, FileMode: -1}
		for _, step := range spec.steps {
			offset = step.offset
			var err error
			switch step.op {
			case "store":
				err = store.Store(step.state, step.provider)
			case "validate":
				err = store.Validate(step.state)
			default:
				fail("unknown state op %q", step.op)
			}
			recorded := stateStep{Op: step.op, State: step.state, Provider: step.provider, At: base.Add(step.offset).Format(time.RFC3339)}
			if err != nil {
				recorded.Error = err.Error()
			}
			caseOut.Steps = append(caseOut.Steps, recorded)
		}
		if path != "" {
			if raw, err := os.ReadFile(path); err == nil {
				caseOut.File = string(raw)
			}
			if info, err := os.Stat(path); err == nil {
				caseOut.FileMode = int(info.Mode().Perm())
			}
		}
		out = append(out, caseOut)
	}
	return out
}

func tokenCases() []tokenCase {
	type spec struct {
		name      string
		operation string
		provider  string
		params    map[string]string
		status    int
		body      string
		failDial  bool
	}
	specs := []spec{
		{
			name: "exchange_gmail", operation: "exchange", provider: "gmail",
			params: map[string]string{
				"code": "auth-code-1", "client_id": "cid", "client_secret": "csecret",
				"redirect_uri": "https://erase.example/callback", "verifier": "pkce-verifier",
			},
			status: 200,
			body:   `{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}`,
		},
		{
			name: "exchange_without_verifier", operation: "exchange", provider: "outlook",
			params: map[string]string{
				"code": "auth-code-2", "client_id": "cid", "client_secret": "csecret",
				"redirect_uri": "http://localhost:8765/callback",
			},
			status: 200,
			body:   `{"access_token":"at-2","expires_in":3600}`,
		},
		{
			name: "refresh_gmail", operation: "refresh", provider: "gmail",
			params: map[string]string{
				"client_id": "cid", "client_secret": "csecret", "refresh_token": "rt-9",
			},
			status: 200,
			body:   `{"access_token":"at-3","expires_in":3599,"scope":"https://mail.google.com/"}`,
		},
		{
			name: "error_status_is_not_echoed", operation: "exchange", provider: "gmail",
			params: map[string]string{
				"code": "bad-code", "client_id": "cid", "client_secret": "csecret",
				"redirect_uri": "https://erase.example/callback",
			},
			status: 400,
			body:   `{"error":"invalid_grant","error_description":"code auth-code-1 already used"}`,
		},
		{
			name: "invalid_json_response", operation: "refresh", provider: "outlook",
			params: map[string]string{
				"client_id": "cid", "client_secret": "csecret", "refresh_token": "rt-1",
			},
			status: 200,
			body:   "<html>not json</html>",
		},
		{
			name: "unknown_provider_never_sends_a_request", operation: "exchange", provider: "posteo",
			params: map[string]string{"code": "auth-code-3", "client_id": "cid", "client_secret": "s"},
			status: 200,
			body:   `{"access_token":"never"}`,
		},
		{
			name: "transport_failure", operation: "refresh", provider: "gmail",
			params:   map[string]string{"client_id": "cid", "client_secret": "s", "refresh_token": "rt-2"},
			status:   200,
			body:     `{"access_token":"never"}`,
			failDial: true,
		},
	}
	out := make([]tokenCase, 0, len(specs))
	for _, spec := range specs {
		recorded := struct {
			Method      string `json:"method"`
			Path        string `json:"path"`
			ContentType string `json:"content_type"`
			Body        string `json:"body"`
		}{}
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			raw, _ := io.ReadAll(r.Body)
			recorded.Method = r.Method
			recorded.Path = r.URL.Path
			recorded.ContentType = r.Header.Get("Content-Type")
			recorded.Body = string(raw)
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(spec.status)
			_, _ = io.WriteString(w, spec.body)
		}))
		transport := http.RoundTripper(rewriteTransport{target: server.URL})
		if spec.failDial {
			transport = failingTransport{}
		}
		client := &email.OAuth2Client{
			HTTPClient: &http.Client{Transport: transport, Timeout: 5 * time.Second},
			States:     email.NewOAuthStateStore(filepath.Join(mustTempDir(), "oauth2_state.json")),
		}
		var result map[string]any
		var err error
		switch spec.operation {
		case "exchange":
			result, err = client.ExchangeCode(context.Background(), spec.provider,
				spec.params["code"], spec.params["client_id"], spec.params["client_secret"],
				spec.params["redirect_uri"], spec.params["verifier"])
		case "refresh":
			result, err = client.RefreshAccessToken(context.Background(), spec.provider,
				spec.params["client_id"], spec.params["client_secret"], spec.params["refresh_token"])
		default:
			fail("unknown operation %q", spec.operation)
		}
		server.Close()
		caseOut := tokenCase{Name: spec.name, Operation: spec.operation, Provider: spec.provider, Params: spec.params}
		caseOut.Response.Status = spec.status
		caseOut.Response.Body = spec.body
		caseOut.TransportFails = spec.failDial
		caseOut.Recorded = recorded
		if err != nil {
			caseOut.Error = err.Error()
		} else {
			caseOut.Result = string(mustMarshal(result))
		}
		out = append(out, caseOut)
	}
	return out
}

type rewriteTransport struct {
	target string
}

func (t rewriteTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	base, err := url.Parse(t.target)
	if err != nil {
		return nil, err
	}
	req.URL.Scheme = base.Scheme
	req.URL.Host = base.Host
	return http.DefaultTransport.RoundTrip(req)
}

type failingTransport struct{}

func (failingTransport) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, fmt.Errorf("dial tcp: connection refused")
}

func mustTempDir() string {
	dir, err := os.MkdirTemp("", "oauth2-oracle-")
	if err != nil {
		fail("temp dir: %v", err)
	}
	return dir
}

func mustMarshal(value any) []byte {
	raw, err := json.Marshal(value)
	if err != nil {
		fail("marshal: %v", err)
	}
	return raw
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
