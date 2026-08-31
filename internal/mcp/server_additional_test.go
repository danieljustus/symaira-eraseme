package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func rpcRequest(t *testing.T, server *Server, method string, params string) *httptest.ResponseRecorder {
	t.Helper()
	body := `{"jsonrpc":"2.0","id":1,"method":"` + method + `","params":` + params + `}`
	req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(body))
	rec := httptest.NewRecorder()
	server.ServeHTTP(rec, req)
	return rec
}

func responseErrorCode(t *testing.T, rec *httptest.ResponseRecorder) int {
	t.Helper()
	var value struct {
		Error struct {
			Code int `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &value); err != nil {
		t.Fatalf("decode response: %v (%s)", err, rec.Body.String())
	}
	return value.Error.Code
}

func TestServeHTTPRejectsNonPost(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	NewServer(nil).ServeHTTP(rec, req)
	if rec.Code != http.StatusMethodNotAllowed || responseErrorCode(t, rec) != -32600 {
		t.Fatalf("GET response = %d %s", rec.Code, rec.Body.String())
	}
}

func TestServeHTTPAuthAndOriginPolicies(t *testing.T) {
	server := NewServerWithOptions(func(context.Context, string, map[string]any) (any, error) {
		return map[string]any{"ok": true}, nil
	}, ServerOptions{
		AuthToken:     "test-token",
		AllowedOrigin: map[string]struct{}{"app.example": {}},
	})
	body := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`

	for _, tc := range []struct {
		name   string
		header string
		origin string
		code   int
	}{
		{"missing auth", "", "", http.StatusUnauthorized},
		{"wrong auth", "Bearer wrong", "", http.StatusUnauthorized},
		{"bad origin", "Bearer test-token", "https://evil.example", http.StatusForbidden},
		{"accepted", "Bearer test-token", "https://app.example", http.StatusOK},
	} {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(body))
			if tc.header != "" {
				req.Header.Set("Authorization", tc.header)
			}
			if tc.origin != "" {
				req.Header.Set("Origin", tc.origin)
			}
			rec := httptest.NewRecorder()
			server.ServeHTTP(rec, req)
			if rec.Code != tc.code {
				t.Fatalf("status = %d, want %d (%s)", rec.Code, tc.code, rec.Body.String())
			}
		})
	}
}

func TestServeHTTPRejectsMalformedAndOversizedRequests(t *testing.T) {
	server := NewServerWithOptions(nil, ServerOptions{MaxBodyBytes: 8})
	for _, tc := range []struct {
		name string
		body string
		code int
	}{
		{"malformed", "{", http.StatusOK},
		{"oversized", `{"jsonrpc":"2.0"}`, http.StatusRequestEntityTooLarge},
	} {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(tc.body))
			rec := httptest.NewRecorder()
			server.ServeHTTP(rec, req)
			if rec.Code != tc.code {
				t.Fatalf("status = %d, want %d", rec.Code, tc.code)
			}
		})
	}
}

func TestServeHTTPBatchAndNotifications(t *testing.T) {
	calls := 0
	server := NewServer(func(context.Context, string, map[string]any) (any, error) {
		calls++
		return "ok", nil
	})
	batch := `[{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]`
	req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(batch))
	rec := httptest.NewRecorder()
	server.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK || !strings.HasPrefix(rec.Body.String(), "[") {
		t.Fatalf("batch response = %d %s", rec.Code, rec.Body.String())
	}

	notification := `{"jsonrpc":"2.0","method":"tools/call","params":{"name":"status","arguments":{}}}`
	req = httptest.NewRequest(http.MethodPost, "/", strings.NewReader(notification))
	rec = httptest.NewRecorder()
	server.ServeHTTP(rec, req)
	if rec.Code != http.StatusNoContent || rec.Body.Len() != 0 {
		t.Fatalf("notification response = %d %q", rec.Code, rec.Body.String())
	}
	if calls != 1 {
		t.Fatalf("handler calls = %d, want one notification dispatch", calls)
	}
}

func TestServeHTTPProtocolValidation(t *testing.T) {
	server := NewServer(func(context.Context, string, map[string]any) (any, error) {
		return "ok", nil
	})
	cases := []struct {
		name   string
		method string
		params string
		code   int
	}{
		{"invalid initialize params", "initialize", `[]`, -32602},
		{"invalid tools list params", "tools/list", `[]`, -32602},
		{"missing call name", "tools/call", `{}`, -32602},
		{"invalid call args", "tools/call", `{"name":"status","arguments":[]}`, -32602},
		{"unknown tool", "tools/call", `{"name":"unknown","arguments":{}}`, -32601},
		{"legacy status alias", "tools/call", `{"name":"status","arguments":{}}`, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := rpcRequest(t, server, tc.method, tc.params)
			if tc.code == 0 {
				if rec.Code != http.StatusOK || responseErrorCode(t, rec) != 0 {
					t.Fatalf("response = %d %s", rec.Code, rec.Body.String())
				}
				return
			}
			if responseErrorCode(t, rec) != tc.code {
				t.Fatalf("error code = %d, want %d (%s)", responseErrorCode(t, rec), tc.code, rec.Body.String())
			}
		})
	}
}

func TestServeHTTPLegacyRedactFileAndHandlerError(t *testing.T) {
	var gotName string
	var gotArgs map[string]any
	server := NewServer(func(_ context.Context, name string, args map[string]any) (any, error) {
		gotName, gotArgs = name, args
		return nil, errors.New("sqlite3: database is locked")
	})
	rec := rpcRequest(t, server, "redact_file", `{"path":"notes.txt"}`)
	if responseErrorCode(t, rec) != -32602 || gotName != "redact_file" || gotArgs["path"] != "notes.txt" {
		t.Fatalf("legacy response = %d %s; handler=%s %#v", responseErrorCode(t, rec), rec.Body.String(), gotName, gotArgs)
	}
}

func TestServeStdioHandlesEOFInvalidJSONAndNotifications(t *testing.T) {
	server := NewServer(func(context.Context, string, map[string]any) (any, error) {
		return "ok", nil
	})
	var out bytes.Buffer
	input := strings.NewReader(`{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}` + "\n" + `{"jsonrpc":"2.0","method":"tools/list","params":{}}` + "\n")
	if err := server.ServeStdio(context.Background(), input, &out); err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 1 {
		t.Fatalf("stdio response lines = %d, want one for notification suppression", len(lines))
	}
	if err := server.ServeStdio(context.Background(), strings.NewReader("{"), io.Discard); err == nil {
		t.Fatal("invalid stdio JSON should return an error")
	}
}

func TestServeHTTPRejectsEmptyBatch(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader("[]"))
	rec := httptest.NewRecorder()
	NewServer(nil).ServeHTTP(rec, req)
	if responseErrorCode(t, rec) != -32600 {
		t.Fatalf("empty batch response = %s", rec.Body.String())
	}
}
