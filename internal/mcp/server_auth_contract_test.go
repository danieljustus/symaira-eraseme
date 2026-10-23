package mcp

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestServeHTTPBearerAuthContract(t *testing.T) {
	expectedToken := "secret-token-12345"
	server := NewServerWithOptions(func(context.Context, string, map[string]any) (any, error) {
		return map[string]any{"ok": true}, nil
	}, ServerOptions{
		AuthToken: expectedToken,
	})
	listBody := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`

	cases := []struct {
		name       string
		headers    []string // multiple values simulate duplicate headers
		wantStatus int
	}{
		{
			name:       "valid token",
			headers:    []string{"Bearer " + expectedToken},
			wantStatus: http.StatusOK,
		},
		{
			name:       "missing auth header",
			headers:    nil,
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "empty auth header",
			headers:    []string{""},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "duplicate headers - two valid",
			headers:    []string{"Bearer " + expectedToken, "Bearer " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "duplicate headers - valid then invalid",
			headers:    []string{"Bearer " + expectedToken, "Bearer wrong"},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "duplicate headers - invalid then valid",
			headers:    []string{"Bearer wrong", "Bearer " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed scheme - lowercase bearer",
			headers:    []string{"bearer " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed scheme - uppercase BEARER",
			headers:    []string{"BEARER " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed scheme - Basic",
			headers:    []string{"Basic " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed scheme - Token",
			headers:    []string{"Token " + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed - Bearer without space",
			headers:    []string{"Bearer" + expectedToken},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed - bare Bearer word",
			headers:    []string{"Bearer"},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "malformed - Bearer with empty token",
			headers:    []string{"Bearer "},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "wrong token same length",
			headers:    []string{"Bearer wrong-token-99999"},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "length mismatch - shorter token",
			headers:    []string{"Bearer secret"},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "length mismatch - longer token with prefix match",
			headers:    []string{"Bearer " + expectedToken + "-extra"},
			wantStatus: http.StatusUnauthorized,
		},
		{
			name:       "length mismatch - substantially longer token",
			headers:    []string{"Bearer " + expectedToken + strings.Repeat("x", 100)},
			wantStatus: http.StatusUnauthorized,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(listBody))
			for _, h := range tc.headers {
				req.Header.Add("Authorization", h)
			}
			rec := httptest.NewRecorder()
			server.ServeHTTP(rec, req)

			if rec.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d; body = %s", rec.Code, tc.wantStatus, rec.Body.String())
			}

			if tc.wantStatus == http.StatusUnauthorized {
				var errResp struct {
					JSONRPC string `json:"jsonrpc"`
					Error   struct {
						Code    int    `json:"code"`
						Message string `json:"message"`
					} `json:"error"`
					ID any `json:"id"`
				}
				if err := json.Unmarshal(rec.Body.Bytes(), &errResp); err != nil {
					t.Fatalf("unmarshal 401 response: %v (%s)", err, rec.Body.String())
				}
				if errResp.JSONRPC != "2.0" || errResp.Error.Code != -32000 || errResp.Error.Message != "Unauthorized" || errResp.ID != nil {
					t.Fatalf("unexpected 401 payload: %#v", errResp)
				}
				// Verify secret tokens are not leaked into the response body
				if strings.Contains(rec.Body.String(), expectedToken) {
					t.Fatalf("response body leaked expected token: %s", rec.Body.String())
				}
			}
		})
	}
}

func TestAuthorizedUnitDirect(t *testing.T) {
	expected := "mcp-token-test-123"

	// Nil request or nil header
	if authorized(nil, expected) {
		t.Fatal("authorized(nil, ...) must be false")
	}
	noHeaderReq := &http.Request{}
	if authorized(noHeaderReq, expected) {
		t.Fatal("authorized with nil Header must be false")
	}

	// Empty expected token
	validReq := httptest.NewRequest(http.MethodPost, "/", nil)
	validReq.Header.Set("Authorization", "Bearer "+expected)
	if authorized(validReq, "") {
		t.Fatal("authorized(..., empty token) must be false")
	}

	// Correct token
	if !authorized(validReq, expected) {
		t.Fatal("authorized with valid header must be true")
	}

	// Wrong token
	badReq := httptest.NewRequest(http.MethodPost, "/", nil)
	badReq.Header.Set("Authorization", "Bearer wrong-token-test-456")
	if authorized(badReq, expected) {
		t.Fatal("authorized with wrong token must be false")
	}
}

func TestConstantTimeCompareDirect(t *testing.T) {
	// Verify behavior of constantTimeCompare
	if constantTimeCompare("", "") {
		t.Fatal("empty expected must return false")
	}
	if constantTimeCompare("secret", "") {
		t.Fatal("empty expected must return false")
	}
	if constantTimeCompare("", "secret") {
		t.Fatal("empty supplied must return false")
	}
	if !constantTimeCompare("secret", "secret") {
		t.Fatal("equal strings must return true")
	}
	if constantTimeCompare("secret1", "secret2") {
		t.Fatal("different content must return false")
	}
	if constantTimeCompare("sec", "secret") {
		t.Fatal("shorter supplied must return false")
	}
	if constantTimeCompare("secret-extra", "secret") {
		t.Fatal("longer supplied must return false")
	}
}
