//go:build projection_fixture

// Command projection_fixture generates or verifies the committed DB-004
// projection contract fixture by replaying the production Go event store.
// It is test-only: it neither changes production Go behavior nor the default
// backend/cutover path.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

const (
	inputSchema  = "symeraseme.event-store.projection-input.v1"
	outputSchema = "symeraseme.event-store.projection-contract.v1"
)

type sourceDigest struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

type oracleProvenance struct {
	Commit    string         `json:"commit"`
	Generator string         `json:"generator"`
	Sources   []sourceDigest `json:"sources"`
}

type fixtureEvent struct {
	ID          int64  `json:"id"`
	RequestID   int64  `json:"request_id"`
	OccurredAt  string `json:"occurred_at"`
	RecordedAt  string `json:"recorded_at"`
	EventType   string `json:"event_type"`
	PayloadJSON string `json:"payload_json"`
	Source      string `json:"source"`
}

type inputCase struct {
	Name       string         `json:"name"`
	Database   string         `json:"database,omitempty"`
	RequestIDs []int64        `json:"request_ids"`
	Events     []fixtureEvent `json:"events,omitempty"`
}

type inputFixture struct {
	Schema string           `json:"schema"`
	Oracle oracleProvenance `json:"oracle"`
	Cases  []inputCase      `json:"cases"`
}

type differentialHarness struct {
	Status  string `json:"status"`
	Blocker string `json:"blocker"`
}

type expectedState struct {
	RequestID   int64                `json:"request_id"`
	State       eventstore.StateJSON `json:"state"`
	CompactJSON string               `json:"compact_json"`
}

type expectedCase struct {
	Name   string          `json:"name"`
	States []expectedState `json:"states"`
}

type outputFixture struct {
	Schema              string              `json:"schema"`
	Oracle              oracleProvenance    `json:"oracle"`
	DifferentialHarness differentialHarness `json:"differential_harness"`
	Cases               []expectedCase      `json:"cases"`
}

func main() {
	verify := flag.Bool("verify", false, "verify the committed generated fixture instead of writing it")
	flag.Parse()
	if flag.NArg() != 0 {
		fatal("usage: go run -tags projection_fixture ./rust-tests/parity/oracle/projection_fixture [--verify]")
	}

	root, err := repositoryRoot()
	if err != nil {
		fatal(err.Error())
	}
	inputPath := filepath.Join(root, "tests", "fixtures", "event-store", "projection-replay-input.json")
	outputPath := filepath.Join(root, "tests", "fixtures", "event-store", "projection-replay-contract.json")
	inputBytes, err := os.ReadFile(inputPath)
	if err != nil {
		fatal(fmt.Sprintf("read input fixture: %v", err))
	}
	var input inputFixture
	if err := json.Unmarshal(inputBytes, &input); err != nil {
		fatal(fmt.Sprintf("parse input fixture: %v", err))
	}
	if input.Schema != inputSchema {
		fatal(fmt.Sprintf("unexpected input schema %q", input.Schema))
	}
	if err := verifySources(root, input.Oracle.Sources); err != nil {
		fatal(err.Error())
	}

	output, err := generate(root, input)
	if err != nil {
		fatal(err.Error())
	}
	generated, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		fatal(fmt.Sprintf("encode generated fixture: %v", err))
	}
	generated = append(generated, '\n')
	if *verify {
		committed, err := os.ReadFile(outputPath)
		if err != nil {
			fatal(fmt.Sprintf("read generated fixture: %v", err))
		}
		if !bytes.Equal(committed, generated) {
			fatal("generated projection fixture differs; regenerate from the production Go oracle")
		}
		return
	}
	if err := os.WriteFile(outputPath, generated, 0o644); err != nil {
		fatal(fmt.Sprintf("write generated fixture: %v", err))
	}
}

func generate(root string, input inputFixture) (outputFixture, error) {
	output := outputFixture{
		Schema: inputSchemaToOutput(input.Schema),
		Oracle: input.Oracle,
		DifferentialHarness: differentialHarness{
			Status:  "blocked",
			Blocker: "For DB cases, the current neutral parity CLI provides only post-execution SQLite snapshots; it has no DB-case manifest, fixture seeding, projection execution, or golden JSON comparator. Adding those is a separate harness slice.",
		},
		Cases: make([]expectedCase, 0, len(input.Cases)),
	}
	for _, testCase := range input.Cases {
		states, err := replayCase(root, testCase)
		if err != nil {
			return outputFixture{}, fmt.Errorf("case %s: %w", testCase.Name, err)
		}
		output.Cases = append(output.Cases, expectedCase{Name: testCase.Name, States: states})
	}
	return output, nil
}

func inputSchemaToOutput(schema string) string {
	if schema != inputSchema {
		return ""
	}
	return outputSchema
}

func replayCase(root string, testCase inputCase) ([]expectedState, error) {
	dir, err := os.MkdirTemp("", "symeraseme-projection-fixture-*")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(dir)
	database := filepath.Join(dir, "projection.db")
	if testCase.Database != "" {
		contents, err := os.ReadFile(filepath.Join(root, "tests", "fixtures", "event-store", testCase.Database))
		if err != nil {
			return nil, err
		}
		if err := os.WriteFile(database, contents, 0o600); err != nil {
			return nil, err
		}
	}

	store, err := eventstore.Open(database)
	if err != nil {
		return nil, err
	}
	defer store.Close()
	if testCase.Database == "" {
		if err := materializeSyntheticCase(store, testCase); err != nil {
			return nil, err
		}
	}

	ctx := context.Background()
	states := make([]expectedState, 0, len(testCase.RequestIDs))
	for _, requestID := range testCase.RequestIDs {
		state, err := store.RebuildState(ctx, requestID)
		if err != nil {
			return nil, err
		}
		compactState, err := json.Marshal(state)
		if err != nil {
			return nil, err
		}
		states = append(states, expectedState{
			RequestID:   requestID,
			State:       state,
			CompactJSON: string(compactState),
		})
	}
	return states, nil
}

func materializeSyntheticCase(store *eventstore.Store, testCase inputCase) error {
	ctx := context.Background()
	if _, err := store.CreateCampaign(ctx, "projection-contract", "initial", ""); err != nil {
		return err
	}
	for _, requestID := range testCase.RequestIDs {
		if _, err := store.DB().ExecContext(ctx,
			`INSERT INTO removal_requests
			 (id, broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash)
			 VALUES (?, ?, 'email', 'projection-contract', 'DE', '', '')`,
			requestID, fmt.Sprintf("projection-broker-%d", requestID)); err != nil {
			return err
		}
	}
	for _, event := range testCase.Events {
		if _, err := store.DB().ExecContext(ctx,
			`INSERT INTO request_events
			 (id, request_id, occurred_at, recorded_at, event_type, payload_json, source)
			 VALUES (?, ?, ?, ?, ?, ?, ?)`,
			event.ID, event.RequestID, event.OccurredAt, event.RecordedAt,
			event.EventType, event.PayloadJSON, event.Source); err != nil {
			return fmt.Errorf("insert fixture event id=%d: %w", event.ID, err)
		}
	}
	return nil
}

func verifySources(root string, sources []sourceDigest) error {
	if len(sources) == 0 {
		return fmt.Errorf("oracle source digest list is empty")
	}
	for _, source := range sources {
		contents, err := os.ReadFile(filepath.Join(root, source.Path))
		if err != nil {
			return fmt.Errorf("read oracle source %s: %w", source.Path, err)
		}
		digest := sha256.Sum256(contents)
		if actual := hex.EncodeToString(digest[:]); actual != source.SHA256 {
			return fmt.Errorf("oracle source drifted: %s", source.Path)
		}
	}
	return nil
}

func repositoryRoot() (string, error) {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		return "", fmt.Errorf("oracle source path unavailable")
	}
	for directory := filepath.Dir(source); ; directory = filepath.Dir(directory) {
		if _, err := os.Stat(filepath.Join(directory, "go.mod")); err == nil {
			return directory, nil
		}
		parent := filepath.Dir(directory)
		if parent == directory {
			return "", fmt.Errorf("repository root not found")
		}
	}
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
