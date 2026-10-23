package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

type testCase struct {
	Name                 string          `json:"name"`
	ExpectedResponseDays json.RawMessage `json:"expected_response_days"`
}

type oracleOutput struct {
	Cases map[string]eventstore.StateJSON `json:"cases"`
}

func main() {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		fatal("oracle source path unavailable")
	}
	input, err := os.ReadFile(filepath.Join(filepath.Dir(source), "cases.json"))
	if err != nil {
		fatal(err.Error())
	}
	var cases []testCase
	if err := json.Unmarshal(input, &cases); err != nil {
		fatal(err.Error())
	}

	root, err := os.MkdirTemp("", "symeraseme-projection-oracle-*")
	if err != nil {
		fatal(err.Error())
	}
	defer os.RemoveAll(root)
	store, err := eventstore.Open(filepath.Join(root, "projection.db"))
	if err != nil {
		fatal(err.Error())
	}
	defer store.Close()
	ctx := context.Background()
	occurred := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	output := oracleOutput{Cases: make(map[string]eventstore.StateJSON, len(cases))}
	for _, test := range cases {
		requestID, err := store.CreateRemovalRequest(ctx, test.Name, "email", "projection-oracle", "DE", "", "")
		if err != nil {
			fatal(err.Error())
		}
		var value any
		if err := json.Unmarshal(test.ExpectedResponseDays, &value); err != nil {
			fatal(err.Error())
		}
		_, _, err = store.AppendAndProject(ctx, requestID, eventstore.EvtSent,
			map[string]any{"expected_response_days": value}, eventstore.SrcSystem, occurred)
		if err != nil {
			fatal(err.Error())
		}
		state, err := store.RebuildState(ctx, requestID)
		if err != nil {
			fatal(err.Error())
		}
		output.Cases[test.Name] = state
	}
	if err := json.NewEncoder(os.Stdout).Encode(output); err != nil {
		fatal(err.Error())
	}
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
