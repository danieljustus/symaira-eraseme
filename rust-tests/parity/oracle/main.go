package main

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-eraseme/internal/confirmation"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore/timeutil"
)

type fixture struct {
	Timestamps []struct {
		Input string `json:"input"`
	} `json:"timestamps"`
	URLs []struct {
		Input string `json:"input"`
	} `json:"urls"`
}

type timestampResult struct {
	Input       string `json:"input"`
	Accepted    bool   `json:"accepted"`
	ErrorClass  string `json:"error_class,omitempty"`
	ISO         string `json:"iso,omitempty"`
	SQL         string `json:"sql,omitempty"`
	SQLBytesHex string `json:"sql_bytes_hex,omitempty"`
}

type urlResult struct {
	Input string   `json:"input"`
	Links []string `json:"links"`
}

type result struct {
	Timestamps []timestampResult `json:"timestamps"`
	URLs       []urlResult       `json:"urls"`
}

func main() {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		fmt.Fprintln(os.Stderr, "oracle source path unavailable")
		os.Exit(1)
	}
	contents, err := os.ReadFile(filepath.Join(filepath.Dir(source), "cases.json"))
	if err != nil {
		fmt.Fprintln(os.Stderr, "oracle fixture unavailable")
		os.Exit(1)
	}
	var cases fixture
	if err := json.Unmarshal(contents, &cases); err != nil {
		fmt.Fprintln(os.Stderr, "oracle fixture is invalid")
		os.Exit(1)
	}

	output := result{
		Timestamps: make([]timestampResult, 0, len(cases.Timestamps)),
		URLs:       make([]urlResult, 0, len(cases.URLs)),
	}
	for _, testCase := range cases.Timestamps {
		row := timestampResult{Input: testCase.Input}
		value, err := timeutil.Parse(testCase.Input)
		if err != nil {
			row.ErrorClass = "malformed"
			if errors.Is(err, timeutil.ErrEmpty) {
				row.ErrorClass = "empty"
			}
		} else {
			row.Accepted = true
			row.ISO = timeutil.FormatISO(value)
			row.SQL = timeutil.FormatSQL(value)
			row.SQLBytesHex = hex.EncodeToString([]byte(row.SQL))
		}
		output.Timestamps = append(output.Timestamps, row)
	}
	for _, testCase := range cases.URLs {
		output.URLs = append(output.URLs, urlResult{
			Input: testCase.Input,
			Links: confirmation.ExtractConfirmationLinks(testCase.Input),
		})
	}
	if err := json.NewEncoder(os.Stdout).Encode(output); err != nil {
		fmt.Fprintln(os.Stderr, "oracle output failed")
		os.Exit(1)
	}
}
