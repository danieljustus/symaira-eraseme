package manualtasks

// ToolDefinitions returns the four frozen manual-task MCP descriptors. The
// transport/server package can embed these definitions without duplicating the
// Python contract.
func ToolDefinitions() []map[string]any {
	return []map[string]any{
		{
			"name":        "manual_tasks_list",
			"description": "List manual fallback tasks for forms that could not be automated.",
			"inputSchema": map[string]any{
				"type": "object",
				"properties": map[string]any{
					"status":     map[string]any{"type": "string", "description": "Filter by task status"},
					"request_id": map[string]any{"type": "integer", "description": "Filter by request ID"},
				},
			},
		},
		{
			"name":        "manual_tasks_show",
			"description": "Show details of a specific manual task.",
			"inputSchema": map[string]any{
				"type": "object",
				"properties": map[string]any{
					"task_id": map[string]any{"type": "integer", "description": "Manual task ID"},
				},
				"required": []string{"task_id"},
			},
		},
		{
			"name":        "manual_tasks_complete",
			"description": "Mark a manual task as completed.",
			"inputSchema": map[string]any{
				"type": "object",
				"properties": map[string]any{
					"task_id": map[string]any{"type": "integer", "description": "Manual task ID"},
					"notes":   map[string]any{"type": "string", "description": "Completion notes", "default": ""},
				},
				"required": []string{"task_id"},
			},
		},
		{
			"name":        "manual_tasks_cleanup",
			"description": "Remove old screenshot and HTML snapshot files from manual tasks.",
			"inputSchema": map[string]any{
				"type": "object",
				"properties": map[string]any{
					"dry_run": map[string]any{"type": "boolean", "description": "Preview without deleting", "default": false},
				},
			},
		},
	}
}
