// Command agent-path records Go host-agent PATH discovery behavior.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type result struct {
	Schema           string            `json:"schema"`
	GoVersion        string            `json:"go_version"`
	GeneratedOn      string            `json:"generated_on"`
	SourcesSHA256    map[string]string `json:"sources_sha256"`
	ID               string            `json:"id"`
	CLI              string            `json:"cli"`
	Path             string            `json:"path"`
	ExecutableExists bool              `json:"executable_exists"`
	Available        bool              `json:"available"`
}

func main() {
	fixture := flag.String("fixture", "tests/fixtures/agent-path/current-relative-entry.json", "fixture output path")
	flag.Parse()
	output, err := filepath.Abs(*fixture)
	fatalIf(err)

	root, err := os.MkdirTemp("", "eraseme-agent-path-oracle-")
	fatalIf(err)
	defer os.RemoveAll(root)
	previousDir, err := os.Getwd()
	fatalIf(err)
	defer func() { fatalIf(os.Chdir(previousDir)) }()
	fatalIf(os.Chdir(root))

	cliFile := "claude"
	if runtime.GOOS == "windows" {
		cliFile = "claude.exe"
	}
	fatalIf(os.WriteFile(cliFile, []byte("fake"), 0o700))
	fatalIf(os.Setenv("PATH", "."))
	fatalIf(os.Setenv("GODEBUG", ""))
	if runtime.GOOS == "windows" {
		fatalIf(os.Setenv("PATHEXT", ".EXE"))
	}

	client, err := llm.Create(llm.CreateOptions{Provider: "agent", AgentBackend: "claude"})
	fatalIf(err)
	agent, ok := client.(*llm.AgentClient)
	if !ok {
		fatalIf(fmt.Errorf("agent provider returned %T", client))
	}
	got := result{
		Schema:           "symeraseme.go-oracle.agent-path.v1",
		GoVersion:        runtime.Version(),
		GeneratedOn:      runtime.GOOS + "/" + runtime.GOARCH,
		SourcesSHA256:    make(map[string]string),
		ID:               "relative-path-hit-is-rejected",
		CLI:              "claude",
		Path:             ".",
		ExecutableExists: true,
		Available:        agent.IsAvailable(),
	}
	for _, source := range []string{"internal/llm/agent.go", "go.mod"} {
		contents, err := os.ReadFile(filepath.Join(previousDir, source))
		fatalIf(err)
		digest := sha256.Sum256(contents)
		got.SourcesSHA256[source] = hex.EncodeToString(digest[:])
	}

	contents, err := json.MarshalIndent(got, "", "  ")
	fatalIf(err)
	fatalIf(os.MkdirAll(filepath.Dir(output), 0o755))
	fatalIf(os.WriteFile(output, append(contents, '\n'), 0o644))
	fmt.Printf("recorded %s available=%t -> %s\n", got.ID, got.Available, output)
}

func fatalIf(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
