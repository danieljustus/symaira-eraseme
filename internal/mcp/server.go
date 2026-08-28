package mcp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
)

// Handler executes one named tool. Implementations must not write to stdout;
// the transport owns the JSON-RPC response stream.
type Handler func(context.Context, string, map[string]any) (any, error)

type Server struct {
	handler Handler
}

func NewServer(handler Handler) *Server {
	if handler == nil {
		handler = func(context.Context, string, map[string]any) (any, error) {
			return nil, errors.New("tool backend is not available")
		}
	}
	return &Server{handler: handler}
}

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Method  string          `json:"method"`
	Params  map[string]any  `json:"params"`
}

type response struct {
	JSONRPC string `json:"jsonrpc"`
	Result  any    `json:"result,omitempty"`
	Error   any    `json:"error,omitempty"`
	ID      any    `json:"id"`
}

type rpcError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

// ServeHTTP exposes the HTTP JSON-RPC contract documented in docs/mcp-contract.md.
func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeRPC(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "POST required"}, ID: nil})
		return
	}
	defer r.Body.Close()
	var raw json.RawMessage
	if err := json.NewDecoder(r.Body).Decode(&raw); err != nil {
		writeRPC(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32700, Message: "parse error"}, ID: nil})
		return
	}
	resp := s.handle(r.Context(), raw)
	if resp == nil { // JSON-RPC notification: no response body.
		w.WriteHeader(http.StatusNoContent)
		return
	}
	writeRPC(w, *resp)
}

func (s *Server) handle(ctx context.Context, raw json.RawMessage) *response {
	var req request
	if err := json.Unmarshal(raw, &req); err != nil {
		return &response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "invalid request"}, ID: nil}
	}
	id := decodeID(req.ID)
	if req.ID == nil || string(req.ID) == "null" {
		id = nil
	}
	if req.JSONRPC != "2.0" || req.Method == "" {
		return &response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "invalid request"}, ID: id}
	}
	if req.Method == "initialize" {
		return &response{JSONRPC: "2.0", Result: map[string]any{"protocolVersion": "2025-06-18", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]any{"name": "symeraseme", "version": "dev"}}, ID: id}
	}
	if req.Method == "tools/list" {
		return &response{JSONRPC: "2.0", Result: map[string]any{"tools": ToolDefinitions()}, ID: id}
	}
	if req.Method == "tools/call" {
		name, _ := req.Params["name"].(string)
		if name == "" {
			return &response{JSONRPC: "2.0", Error: rpcError{Code: -32602, Message: "missing tool name"}, ID: id}
		}
		args, _ := req.Params["arguments"].(map[string]any)
		result, err := s.handler(ctx, name, args)
		if err != nil {
			return &response{JSONRPC: "2.0", Error: rpcError{Code: -32603, Message: sanitizeError(err)}, ID: id}
		}
		return &response{JSONRPC: "2.0", Result: contentEnvelope(result), ID: id}
	}
	if req.Method == "redact_file" {
		result, err := s.handler(ctx, req.Method, req.Params)
		if err != nil {
			return &response{JSONRPC: "2.0", Error: rpcError{Code: -32602, Message: sanitizeError(err)}, ID: id}
		}
		return &response{JSONRPC: "2.0", Result: result, ID: id}
	}
	return &response{JSONRPC: "2.0", Error: rpcError{Code: -32601, Message: "method not found"}, ID: id}
}

func contentEnvelope(result any) map[string]any {
	data, err := json.Marshal(result)
	if err != nil {
		data = []byte(fmt.Sprintf("%v", result))
	}
	return map[string]any{"content": []map[string]string{{"type": "text", "text": string(data)}}}
}

func sanitizeError(err error) string {
	message := err.Error()
	lower := strings.ToLower(message)
	for _, marker := range []string{"sqlite", "no such table", "database is locked"} {
		if strings.Contains(lower, marker) {
			return "Database not ready — start the server again"
		}
	}
	return message
}

func decodeID(raw json.RawMessage) any {
	if len(raw) == 0 || string(raw) == "null" {
		return nil
	}
	var id any
	if json.Unmarshal(raw, &id) == nil {
		return id
	}
	return nil
}

func writeRPC(w http.ResponseWriter, resp response) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_ = json.NewEncoder(w).Encode(resp)
}

// ServeStdio is provided for local integrations that use newline-delimited
// JSON-RPC. It emits only protocol frames and never logs to stdout.
func (s *Server) ServeStdio(ctx context.Context, in io.Reader, out io.Writer) error {
	decoder := json.NewDecoder(in)
	encoder := json.NewEncoder(out)
	for {
		var raw json.RawMessage
		if err := decoder.Decode(&raw); err != nil {
			if errors.Is(err, io.EOF) {
				return nil
			}
			return err
		}
		resp := s.handle(ctx, raw)
		if resp != nil {
			if err := encoder.Encode(resp); err != nil {
				return err
			}
		}
	}
}
