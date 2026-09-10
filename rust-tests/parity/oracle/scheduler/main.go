// Command scheduler is the committed Go scheduler-generation provenance
// oracle.
//
// Unlike the config oracle, scheduler.Generate and scheduler.WriteFiles read
// no environment or filesystem state beyond their explicit Config argument,
// so no isolated-process sandbox is required here: the oracle calls the
// production internal/scheduler package in-process for a fixed set of named
// cases and dumps the resulting byte-exact file maps as JSON.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

const (
	sourceRevision = "ef1f7bcbaf748b490eeeb0bc4ef879cb2f536de6"
	sourceSHA256   = "d936e2fea18f2f36e4801b3812d2c5c5d379c33423680e35c74d3d7a4e5f8860"
	oracleSchema   = "symaira-eraseme.scheduler-parity.v1"
)

type provenance struct {
	SourceRevision string `json:"source_revision"`
	SourcePath     string `json:"source_path"`
	SourceSHA256   string `json:"source_sha256"`
	Schema         string `json:"schema"`
}

type oracleOutput struct {
	Provenance provenance                   `json:"provenance"`
	Cases      map[string]map[string]string `json:"cases"`
}

func main() {
	source := oracleSourcePath()
	schedulerSource := filepath.Clean(filepath.Join(filepath.Dir(source), "../../../../internal/scheduler/scheduler.go"))
	if err := verifySourceHash(schedulerSource, sourceSHA256); err != nil {
		fatal("oracle source provenance verification failed: " + err.Error())
	}

	cases := map[string]scheduler.Config{
		"deterministic_custom_cron": {
			Platform:   scheduler.PlatformCron,
			ProjectDir: "/work/project with spaces",
			BinaryPath: "/opt/symaira/bin/symeraseme",
			TickHour:   6,
			TickMinute: 45,
			PollHours:  []int{8, 20},
		},
		"all_backends_cron": {
			Platform:   scheduler.PlatformCron,
			BinaryPath: "/usr/local/bin/symeraseme",
			ProjectDir: "/srv/erase",
			TickHour:   10,
			PollHours:  []int{8, 12, 16, 20},
		},
		"all_backends_launchd": {
			Platform:   scheduler.PlatformLaunchd,
			BinaryPath: "/usr/local/bin/symeraseme",
			ProjectDir: "/srv/erase",
			TickHour:   10,
			PollHours:  []int{8, 12, 16, 20},
		},
		"all_backends_systemd": {
			Platform:   scheduler.PlatformSystemd,
			BinaryPath: "/usr/local/bin/symeraseme",
			ProjectDir: "/srv/erase",
			TickHour:   10,
			PollHours:  []int{8, 12, 16, 20},
		},
		"default_poll_hours_cron": {
			Platform:   scheduler.PlatformCron,
			BinaryPath: "/usr/local/bin/symeraseme",
			ProjectDir: "/srv/erase",
			TickHour:   10,
			TickMinute: 0,
		},
		"venv_activate_cron": {
			Platform:     scheduler.PlatformCron,
			BinaryPath:   "/usr/local/bin/symeraseme",
			ProjectDir:   "/srv/erase",
			TickHour:     10,
			PollHours:    []int{8, 20},
			VenvActivate: "/srv/erase/venv/bin/activate with spaces'and'quotes",
		},
	}

	caseNames := make([]string, 0, len(cases))
	for name := range cases {
		caseNames = append(caseNames, name)
	}
	sort.Strings(caseNames)

	output := oracleOutput{
		Provenance: provenance{
			SourceRevision: sourceRevision,
			SourcePath:     "internal/scheduler/scheduler.go",
			SourceSHA256:   sourceSHA256,
			Schema:         oracleSchema,
		},
		Cases: make(map[string]map[string]string, len(caseNames)),
	}
	for _, name := range caseNames {
		files, err := scheduler.Generate(cases[name])
		if err != nil {
			fatal(fmt.Sprintf("generate case %s: %v", name, err))
		}
		output.Cases[name] = files
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fatal("write oracle result")
	}
}

// runtimeCaller is kept as a variable so source paths can be tested without
// modifying tracked source files.
var runtimeCaller = func() (uintptr, string, int, bool) {
	return runtime.Caller(0)
}

func oracleSourcePath() string {
	_, source, _, ok := runtimeCaller()
	if !ok {
		fatal("oracle source path unavailable")
	}
	return source
}

func verifySourceHash(path, expected string) error {
	contents, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	digest := sha256.Sum256(contents)
	if hex.EncodeToString(digest[:]) != expected {
		return fmt.Errorf("source hash mismatch")
	}
	return nil
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
