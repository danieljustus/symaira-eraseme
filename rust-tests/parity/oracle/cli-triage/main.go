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
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

const fixturePath = "tests/fixtures/cli-triage/cases.json"

const fakeAgentScript = `#!/bin/sh
case "$*" in
  *"--model oracle-env-model"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"model selected from environment","extracted_fields":{"ticket":"T-42"}}' ;;
  *"--model oracle-flag-model"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"model selected from flag","extracted_fields":{"ticket":"T-42"}}' ;;
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
	ID           string            `json:"id"`
	Argv         []string          `json:"argv"`
	Environment  map[string]string `json:"environment"`
	ExitCode     int               `json:"exit_code"`
	StdoutBase64 string            `json:"stdout_base64"`
	StderrBase64 string            `json:"stderr_base64"`
	State        snapshot          `json:"state"`
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

type commandCase struct {
	id          string
	argv        []string
	environment map[string]string
}

var cases = []commandCase{
	{
		id: "classify-reply-json", argv: []string{"classify-reply", "1", "--provider", "agent", "--output", "json"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "ignored-by-flag", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id: "generate-rebuttal-text", argv: []string{"generate-rebuttal", "1", "--provider", "agent"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "agent", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "classify-save-false-env-provider",
		argv:        []string{"classify-reply", "--request-id=0x1", "--save=false", "--output", "json"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "agent", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "rebuttal-save-false-env-provider",
		argv:        []string{"generate-rebuttal", "--request-id", "1", "--save=false"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "agent", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "classify-positional-overrides-request-id-flag",
		argv:        []string{"classify-reply", "1", "--request-id", "999", "--provider", "agent", "--output", "json"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "ignored-by-flag", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "classify-request-id-flag-hex",
		argv:        []string{"classify-reply", "--request-id", "0x1", "--provider", "agent", "--output", "json"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "ignored-by-flag", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "classify-missing-reply-error",
		argv:        []string{"classify-reply", "999", "--provider", "agent"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "ignored-by-flag", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id: "classify-invalid-provider-error", argv: []string{"classify-reply", "1"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "not-a-provider", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:          "classify-invalid-request-id-flag-error",
		argv:        []string{"classify-reply", "--request-id=bad", "--provider", "agent", "--output", "json"},
		environment: map[string]string{"SYMERASEME_LLM_PROVIDER": "agent", "SYMERASEME_AGENT_BACKEND": "claude"},
	},
	{
		id:   "classify-model-env-default",
		argv: []string{"classify-reply", "1", "--output", "json"},
		environment: map[string]string{
			"SYMERASEME_LLM_PROVIDER": "agent", "SYMERASEME_LLM_MODEL": "oracle-env-model", "SYMERASEME_AGENT_BACKEND": "claude",
		},
	},
	{
		id:   "classify-model-flag-overrides-env",
		argv: []string{"classify-reply", "1", "--provider", "agent", "--model", "oracle-flag-model", "--output", "json"},
		environment: map[string]string{
			"SYMERASEME_LLM_PROVIDER": "not-a-provider", "SYMERASEME_LLM_MODEL": "oracle-env-model", "SYMERASEME_AGENT_BACKEND": "claude",
		},
	},
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
			"TERM=dumb",
		}
		environmentNames := make([]string, 0, len(testCase.environment))
		for name := range testCase.environment {
			environmentNames = append(environmentNames, name)
		}
		sort.Strings(environmentNames)
		for _, name := range environmentNames {
			command.Env = append(command.Env, name+"="+testCase.environment[name])
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
		stderrBytes := normalizeProviderOrder([]byte(stderr.String()))
		result.Cases = append(result.Cases, recordedCase{
			ID: testCase.id, Argv: testCase.argv, Environment: testCase.environment, ExitCode: exitCode,
			StdoutBase64: base64.StdEncoding.EncodeToString([]byte(stdout.String())),
			StderrBase64: base64.StdEncoding.EncodeToString(stderrBytes),
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
	result := snapshot{Events: []event{}}
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

func normalizeProviderOrder(stderr []byte) []byte {
	const marker = "Known providers: "
	text := string(stderr)
	index := strings.Index(text, marker)
	if index < 0 {
		return stderr
	}
	start := index + len(marker)
	end := strings.IndexByte(text[start:], '\n')
	if end < 0 {
		end = len(text) - start
	}
	providers := strings.Split(text[start:start+end], ", ")
	sort.Strings(providers)
	return []byte(text[:start] + strings.Join(providers, ", ") + text[start+end:])
}

func digest(value []byte) string {
	sum := sha256.Sum256(value)
	return hex.EncodeToString(sum[:])
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
