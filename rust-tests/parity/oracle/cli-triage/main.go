// Command cli-triage is a source-bound, local-agent oracle for the two
// `classify-reply` and `generate-rebuttal` CLI wrappers. It builds the actual Go
// binary, seeds a private event store, and puts a deterministic fake Claude
// executable first on PATH. No provider or network request is used.
package main

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

const fixturePath = "tests/fixtures/cli-triage/cases.json"

const fakeAgentScript = `#!/bin/sh
case "$*" in
  *"rejection classifier"*) printf '%s\n' '{"classification":"address_mismatch","confidence":0.91,"summary":"address differs","key_points":[],"jurisdiction":"GDPR"}' ;;
  *"email classifier"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"deletion confirmed","extracted_fields":{"ticket":"T-42"}}' ;;
  *) printf '%s\n' '{"classification":"other","confidence":0.1,"summary":"unexpected prompt","key_points":[],"jurisdiction":"unknown"}' ;;
esac
`

type snapshot struct {
	Classification *string  `json:"classification"`
	Confidence     *float64 `json:"confidence"`
	Summary        *string  `json:"summary"`
	Events         []event  `json:"events"`
}

type event struct {
	RequestID int64  `json:"request_id"`
	EventType string `json:"event_type"`
	Source    string `json:"source"`
	Payload   string `json:"payload_json"`
}

type recordedCase struct {
	ID           string   `json:"id"`
	Argv         []string `json:"argv"`
	ExitCode     int      `json:"exit_code"`
	StdoutBase64 string   `json:"stdout_base64"`
	StderrBase64 string   `json:"stderr_base64"`
	State        snapshot `json:"state"`
}

type source struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

type fixture struct {
	Schema          string         `json:"schema"`
	GeneratorSHA256 string         `json:"generator_sha256"`
	FakeAgentSHA256 string         `json:"fake_agent_sha256"`
	Sources         []source       `json:"sources"`
	Cases           []recordedCase `json:"cases"`
}

var cases = []struct {
	id   string
	argv []string
}{
	{"classify-reply-json", []string{"classify-reply", "1", "--provider", "agent", "--output", "json"}},
	{"generate-rebuttal-text", []string{"generate-rebuttal", "1", "--provider", "agent"}},
}

var sourcePaths = []string{
	"cmd/symeraseme/extra_commands.go",
	"internal/mcp/contract_handler.go",
	"internal/replies/service.go",
	"internal/replies/repository.go",
	"internal/triage/classifier.go",
	"internal/triage/rebuttal.go",
	"internal/llm/agent.go",
	"internal/eventstore/store.go",
}

func main() {
	writeFixture := flag.Bool("fixture", false, "write the recorded cases to the fixture path")
	flag.Parse()
	root, err := os.MkdirTemp("", "eraseme-cli-triage-oracle-")
	if err != nil {
		fail("create oracle root: %v", err)
	}
	defer func() { _ = os.RemoveAll(root) }()

	binary := filepath.Join(root, "symeraseme-go")
	build := exec.Command("go", "build", "-o", binary, "./cmd/symeraseme")
	build.Stdout, build.Stderr = os.Stderr, os.Stderr
	if err := build.Run(); err != nil {
		fail("build Go CLI: %v", err)
	}

	agentDir := filepath.Join(root, "bin")
	if err := os.MkdirAll(agentDir, 0o755); err != nil {
		fail("create fake agent directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(agentDir, "claude"), []byte(fakeAgentScript), 0o755); err != nil {
		fail("write fake agent: %v", err)
	}

	result := fixture{
		Schema:          "symeraseme.go-oracle.cli-triage.v1",
		FakeAgentSHA256: digest([]byte(fakeAgentScript)),
	}
	for _, testCase := range cases {
		caseRoot := filepath.Join(root, "case-"+testCase.id)
		home := filepath.Join(caseRoot, "home")
		dataDir := filepath.Join(caseRoot, "data")
		if err := os.MkdirAll(home, 0o755); err != nil {
			fail("create home: %v", err)
		}
		if err := os.MkdirAll(dataDir, 0o700); err != nil {
			fail("create data dir: %v", err)
		}
		seedStore(dataDir)
		command := exec.Command(binary, testCase.argv...)
		command.Dir = caseRoot
		command.Env = []string{
			"HOME=" + home,
			"PATH=" + agentDir,
			"SYMERASEME_DATA_DIR=" + dataDir,
			"SYMERASEME_LLM_PROVIDER=agent",
			"SYMERASEME_AGENT_BACKEND=claude",
			"TERM=dumb",
		}
		var stdout, stderr strings.Builder
		command.Stdout, command.Stderr = &stdout, &stderr
		exitCode := 0
		if err := command.Run(); err != nil {
			if exitErr, ok := err.(*exec.ExitError); ok {
				exitCode = exitErr.ExitCode()
			} else {
				fail("run %v: %v", testCase.argv, err)
			}
		}
		result.Cases = append(result.Cases, recordedCase{
			ID: testCase.id, Argv: testCase.argv, ExitCode: exitCode,
			StdoutBase64: base64.StdEncoding.EncodeToString([]byte(stdout.String())),
			StderrBase64: base64.StdEncoding.EncodeToString([]byte(stderr.String())),
			State:        readSnapshot(dataDir),
		})
	}

	for _, path := range sourcePaths {
		bytes, err := os.ReadFile(path)
		if err != nil {
			fail("read source %s: %v", path, err)
		}
		result.Sources = append(result.Sources, source{Path: path, SHA256: digest(bytes)})
	}
	generator, err := os.ReadFile("rust-tests/parity/oracle/cli-triage/main.go")
	if err != nil {
		fail("read generator: %v", err)
	}
	result.GeneratorSHA256 = digest(generator)
	encoded, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		fail("encode fixture: %v", err)
	}
	encoded = append(encoded, '\n')
	if *writeFixture {
		if err := os.MkdirAll(filepath.Dir(fixturePath), 0o755); err != nil {
			fail("create fixture directory: %v", err)
		}
		if err := os.WriteFile(fixturePath, encoded, 0o644); err != nil {
			fail("write fixture: %v", err)
		}
		fmt.Fprintf(os.Stderr, "wrote %s\n", fixturePath)
		return
	}
	os.Stdout.Write(encoded)
}

func seedStore(dataDir string) {
	store, err := eventstore.Open(filepath.Join(dataDir, "symeraseme.db"))
	if err != nil {
		fail("open seed store: %v", err)
	}
	defer store.Close()
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "oracle-broker", "email", "oracle-campaign", "DE", "gdpr-art17.de.md.j2", "")
	if err != nil {
		fail("create request: %v", err)
	}
	if _, err := replies.NewRepository(store).InsertReply(ctx, &requestID, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records.", ""); err != nil {
		fail("insert reply: %v", err)
	}
}

func readSnapshot(dataDir string) snapshot {
	store, err := eventstore.Open(filepath.Join(dataDir, "symeraseme.db"))
	if err != nil {
		fail("open result store: %v", err)
	}
	defer store.Close()
	var result snapshot
	if err := store.DB().QueryRow(`SELECT classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE id = 1`).Scan(&result.Classification, &result.Confidence, &result.Summary); err != nil && err != sql.ErrNoRows {
		fail("read reply result: %v", err)
	}
	rows, err := store.DB().Query(`SELECT request_id, event_type, source, payload_json FROM request_events ORDER BY id`)
	if err != nil {
		fail("query result events: %v", err)
	}
	defer rows.Close()
	for rows.Next() {
		var item event
		if err := rows.Scan(&item.RequestID, &item.EventType, &item.Source, &item.Payload); err != nil {
			fail("scan event: %v", err)
		}
		result.Events = append(result.Events, item)
	}
	if err := rows.Err(); err != nil {
		fail("read events: %v", err)
	}
	return result
}

func digest(value []byte) string {
	sum := sha256.Sum256(value)
	return hex.EncodeToString(sum[:])
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
