package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestToolDefinitionsRemainPinned(t *testing.T) {
	tools := ToolDefinitions()
	if len(tools) != 26 {
		t.Fatalf("tool count = %d, want 26", len(tools))
	}
	if tools[0]["name"] == nil {
		t.Fatalf("first tool has no name: %#v", tools[0])
	}
}

func TestHTTPToolsListAndCallUseJSONRPCEnvelope(t *testing.T) {
	s := NewServer(func(_ context.Context, name string, args map[string]any) (any, error) {
		return map[string]any{"tool": name, "args": args}, nil
	})
	listBody := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`
	req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(listBody))
	rec := httptest.NewRecorder()
	s.ServeHTTP(rec, req)
	var list struct {
		Result struct {
			Tools []map[string]any `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &list); err != nil {
		t.Fatal(err)
	}
	if len(list.Result.Tools) != 26 {
		t.Fatalf("tools/list count = %d", len(list.Result.Tools))
	}

	callBody := `{"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"status","arguments":{"json":true}}}`
	req = httptest.NewRequest(http.MethodPost, "/", strings.NewReader(callBody))
	rec = httptest.NewRecorder()
	s.ServeHTTP(rec, req)
	var call response
	if err := json.Unmarshal(rec.Body.Bytes(), &call); err != nil {
		t.Fatal(err)
	}
	if call.Error != nil || call.Result == nil || call.ID != "x" {
		t.Fatalf("unexpected call response: %#v", call)
	}
	if !strings.Contains(string(rec.Body.Bytes()), `"type":"text"`) {
		t.Fatalf("missing content envelope: %s", rec.Body.String())
	}
}

func TestErrorsAreSanitizedAndStdioHasOnlyFrames(t *testing.T) {
	s := NewServer(func(context.Context, string, map[string]any) (any, error) {
		return nil, &testRPCError{message: "sqlite3: database is locked"}
	})
	var out bytes.Buffer
	input := strings.NewReader(`{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}` + "\n")
	if err := s.ServeStdio(context.Background(), input, &out); err != nil {
		t.Fatal(err)
	}
	var got response
	if err := json.Unmarshal(out.Bytes(), &got); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out.String(), "Database not ready") || strings.Contains(out.String(), "sqlite3") {
		t.Fatalf("unsanitized response: %s", out.String())
	}
}

type testRPCError struct{ message string }

func (e *testRPCError) Error() string { return e.message }
