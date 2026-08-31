package mcp

import (
	"strings"
	"testing"
)

func TestMCPParameterAndIDHelpers(t *testing.T) {
	if params, object := decodeParams(nil); !object || len(params) != 0 {
		t.Fatalf("nil params = %#v, object=%v", params, object)
	}
	if params, object := decodeParams([]byte("null")); !object || len(params) != 0 {
		t.Fatalf("null params = %#v, object=%v", params, object)
	}
	if _, object := decodeParams([]byte("[]")); object {
		t.Fatal("array params reported as object")
	}
	if value, ok := objectMap(nil); !ok || len(value) != 0 {
		t.Fatalf("nil object map = %#v, ok=%v", value, ok)
	}
	if _, ok := objectMap([]any{}); ok {
		t.Fatal("array accepted as object map")
	}
	if decodeID([]byte(`"request"`)) != "request" || decodeID([]byte(`2`)) != float64(2) {
		t.Fatal("string and numeric IDs were not decoded")
	}
	if decodeID([]byte(`true`)) != nil || decodeID(nil) != nil {
		t.Fatal("invalid IDs were accepted")
	}
	if got := contentEnvelope("plain"); got["content"] == nil {
		t.Fatal("string content envelope is empty")
	}
	if got := contentEnvelope(map[string]any{"ok": true}); !strings.Contains(got["content"].([]map[string]string)[0]["text"], "ok") {
		t.Fatal("object content envelope lost JSON")
	}
}

func TestMCPArgumentValidationAndJSONKinds(t *testing.T) {
	if err := validateToolArguments("not-a-tool", map[string]any{}); err == nil {
		t.Fatal("unknown tool accepted")
	}
	if err := validateToolArguments("status", map[string]any{}); err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		kind  string
		value any
		want  bool
	}{
		{"string", "x", true},
		{"string", 1, false},
		{"boolean", true, true},
		{"boolean", "true", false},
		{"integer", float64(2), true},
		{"integer", float64(2.5), false},
		{"array", []any{"x"}, true},
		{"array", "x", false},
		{"object", map[string]any{}, true},
	} {
		if got := jsonValueMatches(tc.kind, tc.value); got != tc.want {
			t.Errorf("jsonValueMatches(%q, %#v) = %v, want %v", tc.kind, tc.value, got, tc.want)
		}
	}
}

func TestMCPLegacyPathAndReadJSONValue(t *testing.T) {
	if path, err := legacyPath([]byte(`["notes.txt"]`), nil); err != nil || path != "notes.txt" {
		t.Fatalf("array path = %q, err=%v", path, err)
	}
	if _, err := legacyPath([]byte(`[]`), nil); err == nil {
		t.Fatal("empty array path accepted")
	}
	if path, err := legacyPath([]byte(`{"path":"notes.txt"}`), map[string]any{"path": "notes.txt"}); err != nil || path != "notes.txt" {
		t.Fatalf("object path = %q, err=%v", path, err)
	}
	if _, err := legacyPath(nil, map[string]any{}); err == nil {
		t.Fatal("missing object path accepted")
	}
	if raw, err := readJSONValue(strings.NewReader(`{"ok":true}`), 100); err != nil || string(raw) != `{"ok":true}` {
		t.Fatalf("read JSON = %s, err=%v", raw, err)
	}
	for _, input := range []string{"{", `{"a":1} {"b":2}`} {
		if _, err := readJSONValue(strings.NewReader(input), 100); err == nil {
			t.Fatalf("invalid JSON accepted: %q", input)
		}
	}
	if _, err := readJSONValue(strings.NewReader(strings.Repeat("x", 20)), 8); err == nil {
		t.Fatal("oversized JSON accepted")
	}
}

func TestMCPOriginPolicyDefaultsAndExplicitAllowList(t *testing.T) {
	defaultServer := NewServer(nil)
	for _, origin := range []string{"http://localhost:3000", "http://127.0.0.1:8000", "http://[::1]:8000"} {
		if !defaultServer.allowedOrigin(origin) {
			t.Errorf("default origin %q rejected", origin)
		}
	}
	for _, origin := range []string{"https://evil.example", "not a url"} {
		if defaultServer.allowedOrigin(origin) {
			t.Errorf("default origin %q accepted", origin)
		}
	}
	listed := NewServerWithOptions(nil, ServerOptions{AllowedOrigin: map[string]struct{}{"trusted.example": {}}})
	if !listed.allowedOrigin("https://trusted.example/path") || listed.allowedOrigin("http://localhost") {
		t.Fatal("explicit origin allow-list policy failed")
	}
}

func TestMCPErrorSanitization(t *testing.T) {
	for _, tc := range []struct {
		input string
		want  string
	}{
		{"sqlite: no such table", "Database not ready — start the server again"},
		{"panic: unexpected state", "Internal server error"},
		{"invalid broker id", "invalid broker id"},
	} {
		if got := sanitizeError(&testRPCError{message: tc.input}); got != tc.want {
			t.Errorf("sanitizeError(%q) = %q, want %q", tc.input, got, tc.want)
		}
	}
}
