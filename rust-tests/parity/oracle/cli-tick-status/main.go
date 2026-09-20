// Command cli-tick-status is the byte oracle for the CLI answers of
// `plan status` and `plan tick`.
//
// It builds the real Go CLI, gives every case its own isolated runtime root
// (HOME, XDG config, data directory) whose store was seeded from the frozen rows
// in tests/fixtures/cli-tick-status/seed.sql, and records stdout, stderr and the
// exit code in the established CLI fixture shape.
//
// Every case gets a fresh root on purpose: `plan tick` without `--dry-run`
// mutates the store, so sharing one root would make the cases order-dependent.
//
// Usage: go run ./rust-tests/parity/oracle/cli-tick-status --fixture
package main

import (
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	_ "modernc.org/sqlite"
)

const (
	seedPath    = "tests/fixtures/cli-tick-status/seed.sql"
	fixturePath = "tests/fixtures/cli-tick-status/cases.json"
	sourcePath  = "cmd/symeraseme/real_commands.go:198-245"
)

type recordedCase struct {
	ID           string   `json:"id"`
	Argv         []string `json:"argv"`
	Compare      string   `json:"compare"`
	Normalized   []string `json:"normalized_fields,omitempty"`
	ExitCode     int      `json:"exit_code"`
	StdoutBase64 string   `json:"stdout_base64"`
	StderrBase64 string   `json:"stderr_base64"`
	StdoutBytes  int      `json:"stdout_bytes"`
	StderrBytes  int      `json:"stderr_bytes"`
}

type fixture struct {
	Schema string         `json:"schema"`
	Source string         `json:"source"`
	Seed   string         `json:"seed"`
	Cases  []recordedCase `json:"cases"`
}

// The pinned answers. `compare` says how the Rust replay must read the recorded
// bytes: `exact` for a fully deterministic answer, `json_without_wall_clock` for
// an answer whose only moving part is the reported instant.
var cases = []struct {
	id      string
	argv    []string
	compare string
}{
	{"status-text", []string{"plan", "status"}, "exact"},
	{"status-json", []string{"plan", "status", "--output", "json"}, "json_without_wall_clock"},
	{"tick-dry-run-text", []string{"plan", "tick", "--dry-run"}, "exact"},
	{"tick-dry-run-json", []string{"plan", "tick", "--dry-run", "--output", "json"}, "exact"},
	{"tick-text", []string{"plan", "tick"}, "exact"},
	{"tick-json", []string{"plan", "tick", "--output", "json"}, "exact"},
	{"status-invalid-output", []string{"plan", "status", "--output", "xml"}, "exact"},
	{"tick-invalid-output", []string{"plan", "tick", "--output", "xml"}, "exact"},
}

func main() {
	writeFixture := flag.Bool("fixture", false, "write the recorded cases to the fixture path")
	flag.Parse()

	root, err := os.MkdirTemp("", "eraseme-cli-oracle-")
	if err != nil {
		fail("create oracle root: %v", err)
	}
	defer func() { _ = os.RemoveAll(root) }()

	seed, err := os.ReadFile(seedPath)
	if err != nil {
		fail("read seed: %v", err)
	}

	binary := filepath.Join(root, "symeraseme")
	build := exec.Command("go", "build", "-o", binary, "./cmd/symeraseme")
	build.Stdout = os.Stderr
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		fail("build the CLI: %v", err)
	}

	recorded := make([]recordedCase, 0, len(cases))
	for _, testCase := range cases {
		caseRoot := filepath.Join(root, "case-"+testCase.id)
		dataDir := filepath.Join(caseRoot, "data")
		if err := os.MkdirAll(dataDir, 0o700); err != nil {
			fail("create data dir: %v", err)
		}
		environment := cliEnvironment(caseRoot, dataDir)

		// Warm up the schema the way the product creates it, then apply the
		// frozen rows. The CLI has no seeding flag, so this is the only honest
		// way to hand both implementations the same store.
		warm := exec.Command(binary, "plan", "status")
		warm.Env = environment
		if output, err := warm.CombinedOutput(); err != nil {
			fail("warm up the store: %v: %s", err, output)
		}
		if err := applySeed(filepath.Join(dataDir, "symeraseme.db"), string(seed)); err != nil {
			fail("apply the seed: %v", err)
		}

		command := exec.Command(binary, testCase.argv...)
		command.Env = environment
		var stdout, stderr strings.Builder
		command.Stdout = &stdout
		command.Stderr = &stderr
		exitCode := 0
		if err := command.Run(); err != nil {
			exitError, ok := err.(*exec.ExitError)
			if !ok {
				fail("run %v: %v", testCase.argv, err)
			}
			exitCode = exitError.ExitCode()
		}
		stdoutBytes := []byte(stdout.String())
		stderrBytes := []byte(stderr.String())
		entry := recordedCase{
			ID:           testCase.id,
			Argv:         testCase.argv,
			Compare:      testCase.compare,
			ExitCode:     exitCode,
			StdoutBase64: base64.StdEncoding.EncodeToString(stdoutBytes),
			StderrBase64: base64.StdEncoding.EncodeToString(stderrBytes),
			StdoutBytes:  len(stdoutBytes),
			StderrBytes:  len(stderrBytes),
		}
		if testCase.compare == "json_without_wall_clock" {
			// The reported instant moves on every run, so it is masked in the
			// recorded bytes and its format is asserted by the replay instead.
			masked, err := maskWallClock(stdoutBytes)
			if err != nil {
				fail("mask the wall clock in %s: %v", testCase.id, err)
			}
			entry.Normalized = []string{"as_of"}
			entry.StdoutBase64 = base64.StdEncoding.EncodeToString(masked)
			entry.StdoutBytes = len(masked)
		}
		recorded = append(recorded, entry)
		fmt.Fprintf(os.Stderr, "%-24s exit=%d stdout=%dB stderr=%dB\n", testCase.id, exitCode, len(stdoutBytes), len(stderrBytes))
	}

	encoded, err := json.MarshalIndent(fixture{
		Schema: "symeraseme.go-oracle.cli.v1",
		Source: sourcePath,
		Seed:   seedPath,
		Cases:  recorded,
	}, "", "  ")
	if err != nil {
		fail("encode the fixture: %v", err)
	}
	encoded = append(encoded, '\n')

	if !*writeFixture {
		os.Stdout.Write(encoded)
		return
	}
	if err := os.WriteFile(fixturePath, encoded, 0o644); err != nil {
		fail("write the fixture: %v", err)
	}
	fmt.Fprintf(os.Stderr, "wrote %s with %d cases\n", fixturePath, len(recorded))
}

// maskWallClock replaces the value of the `as_of` key with a placeholder. Only
// that one value changes between runs, so only that one value is masked.
func maskWallClock(payload []byte) ([]byte, error) {
	var decoded map[string]json.RawMessage
	if err := json.Unmarshal(payload, &decoded); err != nil {
		return nil, err
	}
	raw, ok := decoded["as_of"]
	if !ok {
		return nil, fmt.Errorf("the payload has no as_of field")
	}
	var instant string
	if err := json.Unmarshal(raw, &instant); err != nil {
		return nil, err
	}
	quoted, err := json.Marshal(instant)
	if err != nil {
		return nil, err
	}
	masked := strings.Replace(string(payload), string(quoted), `"<TIMESTAMP>"`, 1)
	if masked == string(payload) {
		return nil, fmt.Errorf("the as_of value was not found in the payload")
	}
	return []byte(masked), nil
}

// cliEnvironment isolates the process exactly like the recorded CLI fixture
// does: private home, private XDG root, UTC clock, C locale, empty PATH.
func cliEnvironment(root string, dataDir string) []string {
	home := filepath.Join(root, "home")
	_ = os.MkdirAll(home, 0o700)
	return []string{
		"HOME=" + home,
		"XDG_CONFIG_HOME=" + filepath.Join(root, "xdg"),
		"SYMERASEME_DATA_DIR=" + dataDir,
		"TZ=UTC",
		"LC_ALL=C",
		"PATH=",
	}
}

func applySeed(databasePath string, seed string) error {
	database, err := sql.Open("sqlite", databasePath)
	if err != nil {
		return err
	}
	defer func() { _ = database.Close() }()
	statements := make([]string, 0, 16)
	for _, line := range strings.Split(seed, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "--") {
			continue
		}
		statements = append(statements, line)
	}
	// The file order is significant: parents before the rows that reference them.
	for _, statement := range statements {
		if _, err := database.Exec(statement); err != nil {
			return fmt.Errorf("%s: %w", statement, err)
		}
	}
	return nil
}

func fail(format string, arguments ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", arguments...)
	os.Exit(1)
}
