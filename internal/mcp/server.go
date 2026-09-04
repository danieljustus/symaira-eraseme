package mcp

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
)

// Handler executes one named tool. Implementations must not write to stdout;
// the transport owns the JSON-RPC response stream.
type Handler func(context.Context, string, map[string]any) (any, error)

// ServerOptions configures the HTTP transport. An empty AuthToken disables
// authentication, which is useful for embedding and tests. The mcp command
// supplies a token when it exposes a non-loopback HTTP endpoint.
type ServerOptions struct {
	AuthToken     string
	MaxBodyBytes  int64
	AllowRemote   bool
	AllowedOrigin map[string]struct{}
}

type Server struct {
	handler Handler
	options ServerOptions
}

func NewServer(handler Handler) *Server {
	return NewServerWithOptions(handler, ServerOptions{})
}

func NewServerWithOptions(handler Handler, options ServerOptions) *Server {
	if handler == nil {
		handler = func(context.Context, string, map[string]any) (any, error) {
			return nil, errors.New("tool backend is not available")
		}
	}
	if options.MaxBodyBytes <= 0 {
		options.MaxBodyBytes = 5 * 1024 * 1024
	}
	return &Server{handler: handler, options: options}
}

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
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
		writeRPCStatus(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "POST required"}, ID: nil}, http.StatusMethodNotAllowed)
		return
	}
	if origin := r.Header.Get("Origin"); origin != "" && !s.allowedOrigin(origin) {
		writeRPCStatus(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32000, Message: "Forbidden: disallowed Origin"}, ID: nil}, http.StatusForbidden)
		return
	}
	if s.options.AuthToken != "" && !authorized(r, s.options.AuthToken) {
		writeRPCStatus(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32000, Message: "Unauthorized"}, ID: nil}, http.StatusUnauthorized)
		return
	}
	if r.ContentLength > s.options.MaxBodyBytes {
		writeRPCStatus(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "Invalid Request"}, ID: nil}, http.StatusRequestEntityTooLarge)
		return
	}
	defer r.Body.Close()
	body := io.Reader(r.Body)
	if r.ContentLength < 0 {
		body = io.LimitReader(r.Body, s.options.MaxBodyBytes+1)
	}
	raw, err := readJSONValue(body, s.options.MaxBodyBytes)
	if err != nil {
		writeRPC(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32700, Message: "parse error"}, ID: nil})
		return
	}
	if len(raw) > 0 && raw[0] == '[' {
		var batch []json.RawMessage
		if err := json.Unmarshal(raw, &batch); err != nil || len(batch) == 0 {
			writeRPC(w, response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "invalid request"}, ID: nil})
			return
		}
		responses := make([]response, 0, len(batch))
		for _, item := range batch {
			if resp := s.handle(r.Context(), item); resp != nil {
				responses = append(responses, *resp)
			}
		}
		if len(responses) == 0 {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		writeRPC(w, responses)
		return
	}
	resp := s.handle(r.Context(), raw)
	if resp == nil { // JSON-RPC notification: no response body.
		w.WriteHeader(http.StatusNoContent)
		return
	}
	writeRPC(w, *resp)
}

func (s *Server) allowedOrigin(raw string) bool {
	if len(s.options.AllowedOrigin) > 0 {
		parsed, err := url.Parse(raw)
		if err != nil {
			return false
		}
		_, ok := s.options.AllowedOrigin[strings.ToLower(parsed.Hostname())]
		return ok
	}
	parsed, err := url.Parse(raw)
	if err != nil {
		return false
	}
	switch strings.ToLower(parsed.Hostname()) {
	case "localhost", "127.0.0.1", "::1":
		return true
	default:
		return false
	}
}

func authorized(r *http.Request, token string) bool {
	if token == "" || r == nil || r.Header == nil {
		return false
	}
	headers := r.Header.Values("Authorization")
	if len(headers) != 1 {
		return false
	}
	value := headers[0]
	const prefix = "Bearer "
	if !strings.HasPrefix(value, prefix) {
		return false
	}
	supplied := value[len(prefix):]
	if len(supplied) == 0 {
		return false
	}
	return constantTimeCompare(supplied, token)
}

func constantTimeCompare(supplied, expected string) bool {
	suppliedBytes := []byte(supplied)
	expectedBytes := []byte(expected)
	if len(expectedBytes) == 0 {
		return false
	}
	lengthMatch := subtle.ConstantTimeEq(int32(len(suppliedBytes)), int32(len(expectedBytes)))
	toCompare := suppliedBytes
	if lengthMatch == 0 {
		toCompare = expectedBytes
	}
	contentMatch := subtle.ConstantTimeCompare(toCompare, expectedBytes)
	return (lengthMatch & contentMatch) == 1
}

func (s *Server) handle(ctx context.Context, raw json.RawMessage) *response {
	var req request
	if err := json.Unmarshal(raw, &req); err != nil {
		return &response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "invalid request"}, ID: nil}
	}
	id := decodeID(req.ID)
	validID := len(req.ID) == 0 || string(req.ID) == "null" || id != nil
	if req.JSONRPC != "2.0" || req.Method == "" || !validID {
		return &response{JSONRPC: "2.0", Error: rpcError{Code: -32600, Message: "invalid request"}, ID: id}
	}
	// A valid request without an id is a notification. It is still dispatched,
	// but JSON-RPC requires that no response be emitted, including errors.
	notification := len(req.ID) == 0
	resp := s.handleRequest(ctx, req, id)
	if notification {
		return nil
	}
	return resp
}

func (s *Server) handleRequest(ctx context.Context, req request, id any) *response {
	params, object := decodeParams(req.Params)
	switch req.Method {
	case "initialize":
		if req.Params != nil && !object {
			return rpcResponseError(-32602, "invalid params", id)
		}
		return &response{JSONRPC: "2.0", Result: map[string]any{"protocolVersion": "2025-06-18", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]any{"name": "symeraseme", "version": "dev"}}, ID: id}
	case "tools/list", "list_tools":
		if req.Params != nil && !object {
			return rpcResponseError(-32602, "invalid params", id)
		}
		return &response{JSONRPC: "2.0", Result: map[string]any{"tools": ToolDefinitions()}, ID: id}
	case "tools/call":
		if !object {
			return rpcResponseError(-32602, "invalid params", id)
		}
		name, ok := params["name"].(string)
		if !ok || strings.TrimSpace(name) == "" {
			return rpcResponseError(-32602, "missing tool name", id)
		}
		args, ok := objectMap(params["arguments"])
		if !ok {
			return rpcResponseError(-32602, "invalid params", id)
		}
		// Keep the historical status alias accepted for existing clients. It is
		// intentionally not added to the pinned tools catalogue.
		if _, ok := toolByName(name); !ok && name != "status" {
			return rpcResponseError(-32601, "method not found", id)
		}
		if err := validateToolArguments(name, args); err != nil {
			return rpcResponseError(-32602, err.Error(), id)
		}
		result, err := s.handler(ctx, name, args)
		if err != nil {
			return rpcResponseError(-32603, sanitizeError(err), id)
		}
		return &response{JSONRPC: "2.0", Result: contentEnvelope(result), ID: id}
	case "redact_file":
		path, err := legacyPath(req.Params, params)
		if err != nil {
			return rpcResponseError(-32602, err.Error(), id)
		}
		result, err := s.handler(ctx, "redact_file", map[string]any{"path": path})
		if err != nil {
			return rpcResponseError(-32602, sanitizeError(err), id)
		}
		return &response{JSONRPC: "2.0", Result: result, ID: id}
	default:
		return rpcResponseError(-32601, "method not found", id)
	}
}

func legacyPath(raw json.RawMessage, params map[string]any) (string, error) {
	if len(raw) > 0 && raw[0] == '[' {
		var values []any
		if err := json.Unmarshal(raw, &values); err == nil && len(values) > 0 {
			if path, ok := values[0].(string); ok && path != "" {
				return path, nil
			}
		}
		return "", errors.New("missing path parameter")
	}
	path, ok := params["path"].(string)
	if !ok || path == "" {
		return "", errors.New("missing path parameter")
	}
	return path, nil
}

func decodeParams(raw json.RawMessage) (map[string]any, bool) {
	if len(raw) == 0 || string(raw) == "null" {
		return map[string]any{}, len(raw) == 0 || string(raw) == "null"
	}
	var params map[string]any
	if json.Unmarshal(raw, &params) != nil || params == nil {
		return nil, false
	}
	return params, true
}

func objectMap(value any) (map[string]any, bool) {
	if value == nil {
		return map[string]any{}, true
	}
	params, ok := value.(map[string]any)
	return params, ok && params != nil
}

func toolByName(name string) (map[string]any, bool) {
	for _, tool := range ToolDefinitions() {
		if tool["name"] == name {
			return tool, true
		}
	}
	return nil, false
}

func validateToolArguments(name string, args map[string]any) error {
	tool, ok := toolByName(name)
	if !ok {
		if name == "status" {
			return nil
		}
		return errors.New("method not found")
	}
	schema, _ := tool["inputSchema"].(map[string]any)
	required, _ := schema["required"].([]any)
	for _, value := range required {
		key, _ := value.(string)
		if _, exists := args[key]; !exists {
			return fmt.Errorf("missing required argument: %s", key)
		}
	}
	properties, _ := schema["properties"].(map[string]any)
	for key, value := range args {
		property, _ := properties[key].(map[string]any)
		kind, _ := property["type"].(string)
		if kind != "" && !jsonValueMatches(kind, value) {
			return fmt.Errorf("invalid parameter type: %s", key)
		}
	}
	return nil
}

func jsonValueMatches(kind string, value any) bool {
	switch kind {
	case "string":
		_, ok := value.(string)
		return ok
	case "boolean":
		_, ok := value.(bool)
		return ok
	case "integer":
		n, ok := value.(float64)
		return ok && n == float64(int64(n))
	case "array":
		_, ok := value.([]any)
		return ok
	default:
		return true
	}
}

func rpcResponseError(code int, message string, id any) *response {
	return &response{JSONRPC: "2.0", Error: rpcError{Code: code, Message: message}, ID: id}
}

func contentEnvelope(result any) map[string]any {
	var text string
	if value, ok := result.(string); ok {
		text = value
	} else {
		data, err := json.Marshal(result)
		if err != nil {
			text = fmt.Sprintf("%v", result)
		} else {
			text = string(data)
		}
	}
	return map[string]any{"content": []map[string]string{{"type": "text", "text": text}}}
}

func sanitizeError(err error) string {
	message := err.Error()
	lower := strings.ToLower(message)
	for _, marker := range []string{"sqlite", "no such table", "database is locked", "database disk image is malformed"} {
		if strings.Contains(lower, marker) {
			return "Database not ready — start the server again"
		}
	}
	if strings.Contains(lower, "panic") {
		return "Internal server error"
	}
	return message
}

func decodeID(raw json.RawMessage) any {
	if len(raw) == 0 || string(raw) == "null" {
		return nil
	}
	var id any
	if json.Unmarshal(raw, &id) == nil {
		switch id.(type) {
		case string, float64:
			return id
		}
	}
	return nil
}

func readJSONValue(in io.Reader, max int64) (json.RawMessage, error) {
	data, err := io.ReadAll(io.LimitReader(in, max+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > max {
		return nil, errors.New("request body too large")
	}
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	var raw json.RawMessage
	if err := decoder.Decode(&raw); err != nil {
		return nil, err
	}
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		if err == nil {
			return nil, errors.New("multiple JSON values")
		}
		return nil, err
	}
	return raw, nil
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

func writeRPC(w http.ResponseWriter, resp any) { writeRPCStatus(w, resp, http.StatusOK) }

func writeRPCStatus(w http.ResponseWriter, resp any, status int) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(resp)
}
