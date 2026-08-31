package mcp

import (
	"context"
	"errors"
)

// ContractHandler returns a safe error for tools whose side-effecting backend
// is not available in the standalone Go binary. Keeping every catalog name in
// the dispatch table prevents clients from seeing an ambiguous method-not-found
// error and makes the missing integration explicit.
func ContractHandler() Handler {
	known := make(map[string]struct{}, len(ToolDefinitions()))
	for _, tool := range ToolDefinitions() {
		if name, ok := tool["name"].(string); ok {
			known[name] = struct{}{}
		}
	}
	return func(_ context.Context, name string, _ map[string]any) (any, error) {
		if _, ok := known[name]; !ok {
			return nil, errors.New("tool is not part of the embedded catalogue")
		}
		return nil, errors.New("tool backend is not available in the standalone Go port")
	}
}
