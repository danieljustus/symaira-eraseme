// Command mcp-triage runs Go's real MCP handler with a private event store and
// fake local agent. It never calls a paid or network LLM provider.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

type observation struct {
	Name          string         `json:"name"`
	Arguments     map[string]any `json:"arguments"`
	WireArguments string         `json:"wire_arguments,omitempty"`
	Result        string         `json:"result,omitempty"`
	Error         string         `json:"error,omitempty"`
}

func main() {
	root, err := os.MkdirTemp("", "eraseme-mcp-triage-")
	must(err)
	defer os.RemoveAll(root)
	data := filepath.Join(root, "data")
	bin := filepath.Join(root, "bin")
	must(os.MkdirAll(data, 0700))
	must(os.MkdirAll(bin, 0700))
	agent, err := os.ReadFile("rust-tests/parity/oracle/mcp-triage/agent.sh")
	must(err)
	must(os.WriteFile(filepath.Join(bin, "claude"), agent, 0755))
	must(os.Setenv("HOME", root))
	must(os.Setenv("PATH", bin))
	must(os.Setenv("SYMERASEME_DATA_DIR", data))
	must(os.Setenv("SYMERASEME_LLM_PROVIDER", "agent"))
	must(os.Setenv("SYMERASEME_AGENT_BACKEND", "claude"))
	store, err := eventstore.Open(filepath.Join(data, "symeraseme.db"))
	must(err)
	ctx := context.Background()
	id, err := store.CreateRemovalRequest(ctx, "oracle-broker", "email", "oracle-campaign", "DE", "gdpr-art17.de.md.j2", "")
	must(err)
	_, err = replies.NewRepository(store).InsertReply(ctx, &id, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records.", "")
	must(err)
	must(store.Close())
	handler := mcp.ContractHandler()
	cases := []observation{
		{Name: "classify_reply", Arguments: map[string]any{"request_id": id, "save": false}},
		{Name: "classify_reply", Arguments: map[string]any{"request_id": float64(id), "save": false}, WireArguments: `{"request_id":1.0,"save":false}`},
		{Name: "classify_reply", Arguments: map[string]any{"request_id": float64(9007199254740993), "save": false}, WireArguments: `{"request_id":9007199254740993,"save":false}`},
		{Name: "generate_rebuttal", Arguments: map[string]any{"request_id": id, "save": false}},
	}
	for i := range cases {
		result, err := handler(ctx, cases[i].Name, cases[i].Arguments)
		if err != nil {
			cases[i].Error = err.Error()
			continue
		}
		encoded, err := json.Marshal(result)
		must(err)
		cases[i].Result = string(encoded)
	}
	must(json.NewEncoder(os.Stdout).Encode(cases))
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
