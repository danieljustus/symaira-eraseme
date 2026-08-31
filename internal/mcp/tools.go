package mcp

import (
	_ "embed"
	"encoding/json"
	"fmt"
)

//go:embed tools.json
var toolDefinitionsJSON []byte

// ToolDefinitions returns a copy of the pinned 26-tool catalogue. The source
// is the same golden document used by the Python contract tests, so schema
// drift is visible in one place rather than being silently recreated in Go.
func ToolDefinitions() []map[string]any {
	var document struct {
		Tools []map[string]any `json:"tools"`
	}
	if err := json.Unmarshal(toolDefinitionsJSON, &document); err != nil {
		panic(fmt.Sprintf("mcp: invalid embedded tool catalogue: %v", err))
	}
	return cloneTools(document.Tools)
}

func cloneTools(tools []map[string]any) []map[string]any {
	out := make([]map[string]any, 0, len(tools))
	for _, tool := range tools {
		data, _ := json.Marshal(tool)
		var copy map[string]any
		if err := json.Unmarshal(data, &copy); err != nil {
			panic(fmt.Sprintf("mcp: invalid tool definition: %v", err))
		}
		out = append(out, copy)
	}
	return out
}
