// Native fake host agent and live Go PATH oracle. No external service is used.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--lookup" {
		sources := map[string]string{}
		for _, source := range []string{"go.mod", "internal/llm/agent.go", "rust-tests/parity/oracle/native-host-agent/main.go"} {
			contents, err := os.ReadFile(filepath.Join(os.Getenv("SYMERASEME_ORACLE_SOURCE_ROOT"), source))
			if err != nil {
				panic(err)
			}
			digest := sha256.Sum256(contents)
			sources[source] = hex.EncodeToString(digest[:])
		}
		path, err := exec.LookPath("claude")
		observation := struct {
			SourcesSHA256 map[string]string `json:"sources_sha256"`
			Found         bool              `json:"found"`
			Available     bool              `json:"available"`
			Path          string            `json:"path"`
		}{sources, err == nil, llm.NewAgentClient("auto", "claude", nil).IsAvailable(), path}
		if err := json.NewEncoder(os.Stdout).Encode(observation); err != nil {
			panic(err)
		}
		return
	}
	started := os.Getenv("AGENT_STARTED")
	if started == "" {
		panic("AGENT_STARTED is required for the synthetic blocking agent")
	}
	// Rename a completed file so the reader never observes a partial PID.
	temporary := filepath.Join(filepath.Dir(started), "pid.tmp")
	if err := os.WriteFile(temporary, []byte(fmt.Sprintf("%d\n", os.Getpid())), 0o600); err != nil {
		panic(err)
	}
	if err := os.Rename(temporary, started); err != nil {
		panic(err)
	}
	time.Sleep(30 * time.Second)
}
