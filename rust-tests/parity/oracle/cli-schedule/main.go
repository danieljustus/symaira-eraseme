// Command cli-schedule is the byte oracle for the CLI answers of
// `schedule install`, `schedule uninstall` and `schedule status`.
//
// It builds the real Go CLI and gives every case its own isolated runtime root
// (HOME, XDG config, data directory) whose scheduler directories are seeded from
// the case list, then records stdout, stderr and the exit code in the
// established CLI fixture shape.
//
// PATH is deliberately empty. That keeps the oracle host-free: `launchctl`,
// `systemctl` and `crontab` are then simply not found, so no real scheduler is
// touched and the recorded failure is the CLI's own, deterministic answer
// instead of this machine's launchd state.
//
// Usage: go run ./rust-tests/parity/oracle/cli-schedule --fixture
package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

const (
	fixturePath = "tests/fixtures/cli-schedule/cases.json"
	sourcePath  = "cmd/symeraseme/extra_commands.go:512-594"
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
	// Files maps a path relative to the case root to the sha256 of its content,
	// so the replay can prove the same bytes landed in the same places.
	Files map[string]string `json:"files"`
}

type fixture struct {
	Schema string         `json:"schema"`
	Source string         `json:"source"`
	Cases  []recordedCase `json:"cases"`
}

// seed says which pre-existing native units a case starts with, so both
// implementations see the same host state.
type seed int

const (
	seedNone seed = iota
	// seedLabeledUnits writes the launchd units under the label names Uninstall
	// and the legacy scan use.
	seedLabeledUnits
	// seedStatusNamedUnits writes the units under the bare names Status resolves.
	seedStatusNamedUnits
	// seedInstallDir writes the generated wrapper directory a completed install
	// leaves behind.
	seedInstallDir
)

var cases = []struct {
	id      string
	argv    []string
	compare string
	seed    seed
}{
	{"install-dry-run-text", []string{"schedule", "install", "--platform", "launchd", "--dry-run"}, "exact", seedNone},
	{"install-dry-run-json", []string{"schedule", "install", "--platform", "launchd", "--dry-run", "--output", "json"}, "exact", seedNone},
	{"install-unsupported-platform", []string{"schedule", "install", "--platform", "windows"}, "exact", seedNone},
	{"install-launchd-without-launchctl", []string{"schedule", "install", "--platform", "launchd"}, "exact", seedNone},
	{"install-launchd-refuses-python-legacy", []string{"schedule", "install", "--platform", "launchd"}, "exact", seedLabeledUnits},
	// The same seed, with the consent flag: the refusal must turn into an
	// attempt, which is what `--replace-legacy` exists to grant. Without the
	// flag the run above stops before writing anything.
	{"install-launchd-replaces-python-legacy", []string{"schedule", "install", "--platform", "launchd", "--replace-legacy"}, "exact", seedLabeledUnits},
	{"status-launchd-without-launchctl", []string{"schedule", "status", "--platform", "launchd"}, "exact", seedStatusNamedUnits},
	{"status-launchd-json", []string{"schedule", "status", "--platform", "launchd", "--output", "json"}, "exact", seedStatusNamedUnits},
	{"status-unsupported-platform", []string{"schedule", "status", "--platform", "windows"}, "exact", seedNone},
	{"status-invalid-output", []string{"schedule", "status", "--platform", "launchd", "--output", "xml"}, "exact", seedNone},
	{"uninstall-launchd-without-launchctl", []string{"schedule", "uninstall", "--platform", "launchd"}, "exact", seedLabeledUnits},
	{"uninstall-launchd-json", []string{"schedule", "uninstall", "--platform", "launchd", "--output", "json"}, "exact", seedLabeledUnits},
	{"uninstall-unsupported-platform", []string{"schedule", "uninstall", "--platform", "windows"}, "exact", seedNone},
}

func main() {
	writeFixture := flag.Bool("fixture", false, "write the recorded cases to the fixture path")
	flag.Parse()

	root, err := os.MkdirTemp("", "eraseme-cli-schedule-oracle-")
	if err != nil {
		fail("create oracle root: %v", err)
	}
	defer func() { _ = os.RemoveAll(root) }()

	binary := oracleBinary(root)
	build := exec.Command("go", "build", "-o", binary, "./cmd/symeraseme")
	build.Stdout = os.Stderr
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		fail("build the CLI: %v", err)
	}

	recorded := make([]recordedCase, 0, len(cases))
	for _, testCase := range cases {
		caseRoot := filepath.Join(root, "case-"+testCase.id)
		home := filepath.Join(caseRoot, "home")
		if err := os.MkdirAll(home, 0o755); err != nil {
			fail("create home: %v", err)
		}
		if err := seedCase(home, testCase.seed); err != nil {
			fail("seed %s: %v", testCase.id, err)
		}
		environment := cliEnvironment(caseRoot)

		command := exec.Command(binary, testCase.argv...)
		// The CLI derives its default project directory from the working
		// directory, which would otherwise be this oracle's own cwd — a
		// machine-specific path leaking into the recorded bytes. Running inside
		// the case root makes it deterministic and folds to <CASE>.
		command.Dir = caseRoot
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
		// The oracle root is volatile and appears inside the generated units and
		// in every reported path, so it is folded to a placeholder in the
		// recorded bytes. Nothing else is masked: the answers themselves stay
		// byte-exact, which is what makes each case meaningful.
		stdoutBytes := []byte(fold(stdout.String(), root, caseRoot))
		stderrBytes := []byte(fold(stderr.String(), root, caseRoot))

		files, err := captureFiles(caseRoot, root)
		if err != nil {
			fail("capture %s: %v", testCase.id, err)
		}

		recorded = append(recorded, recordedCase{
			ID:      testCase.id,
			Argv:    testCase.argv,
			Compare: testCase.compare,
			// The volatile root path and the CLI's own resolved executable path
			// are the only masked values; both are folded to named placeholders
			// and their formats are asserted by the replay.
			// The volatile root path is always folded. The CLI's own resolved
			// executable path is folded too, but only where it actually appears,
			// so the recorded marker reflects what the case really contains.
			Normalized:   normalizedFields(stdoutBytes),
			ExitCode:     exitCode,
			StdoutBase64: base64.StdEncoding.EncodeToString(stdoutBytes),
			StderrBase64: base64.StdEncoding.EncodeToString(stderrBytes),
			StdoutBytes:  len(stdoutBytes),
			StderrBytes:  len(stderrBytes),
			Files:        files,
		})
		fmt.Fprintf(os.Stderr, "%-40s exit=%d stdout=%dB stderr=%dB files=%d\n", testCase.id, exitCode, len(stdoutBytes), len(stderrBytes), len(files))
	}

	encoded, err := json.MarshalIndent(fixture{
		Schema: "symeraseme.go-oracle.cli.v1",
		Source: sourcePath,
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
	if err := os.MkdirAll(filepath.Dir(fixturePath), 0o755); err != nil {
		fail("create fixture dir: %v", err)
	}
	if err := os.WriteFile(fixturePath, encoded, 0o644); err != nil {
		fail("write the fixture: %v", err)
	}
	fmt.Fprintf(os.Stderr, "wrote %s with %d cases\n", fixturePath, len(recorded))
}

// seedCase plants the pre-existing units a case runs against.
func seedCase(home string, which seed) error {
	switch which {
	case seedNone:
		return nil
	case seedLabeledUnits:
		return writeUnits(filepath.Join(home, "Library", "LaunchAgents"), []string{
			"com.symeraseme.tick.plist",
			"com.symeraseme.poll.plist",
			"com.symeraseme.rescan.plist",
		}, "# Generated by symeraseme generate-scheduler\n/usr/bin/python3 -m symeraseme.core.scheduler\n")
	case seedStatusNamedUnits, seedInstallDir:
		// A completed install leaves the units under the bare names Status
		// resolves; the same bytes serve both cases.
		return writeUnits(filepath.Join(home, "Library", "LaunchAgents"), []string{
			"symeraseme-tick.plist",
			"symeraseme-poll.plist",
			"symeraseme-rescan.plist",
		}, "# Generated by symeraseme generate-scheduler (Go)\n")
	}
	return fmt.Errorf("unknown seed %d", which)
}

func writeUnits(dir string, names []string, content string) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			return err
		}
	}
	return nil
}

// normalizedFields names the values a case masks. The root path is always
// folded; the binary path is listed only when the payload really carries it.
func normalizedFields(payload []byte) []string {
	fields := []string{"root_path"}
	if strings.Contains(string(payload), "<BINARY>") {
		fields = append(fields, "binary_path")
	}
	return fields
}

// fold replaces the volatile oracle root and the per-case root with stable
// placeholders. Only those two prefixes are rewritten; the rest of the payload
// is preserved byte for byte.
func fold(value string, root string, caseRoot string) string {
	// The CLI resolves its own executable path into the generated wrappers, so
	// that value is machine-specific by design. It is folded to a named
	// placeholder here and its format is asserted by the replay instead.
	for _, item := range []struct{ path, placeholder string }{
		{oracleBinary(root), "<BINARY>"},
		{caseRoot, "<CASE>"},
		{root, "<ROOT>"},
	} {
		// A JSON string escapes Windows backslashes; the plain path occurs in
		// generated files. Fold both spellings without changing their suffixes.
		value = strings.ReplaceAll(value, strings.ReplaceAll(item.path, `\`, `\\`), item.placeholder)
		value = strings.ReplaceAll(value, item.path, item.placeholder)
	}
	return value
}

func oracleBinary(root string) string {
	name := "symeraseme"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(root, name)
}

// captureFiles walks the case root and hashes every regular file, so the replay
// can prove the same bytes were written to the same relative paths. Contents are
// folded the same way the recorded output is, since generated units embed the
// volatile install paths.
func captureFiles(root string, oracleRoot string) (map[string]string, error) {
	files := map[string]string{}
	err := filepath.Walk(root, func(path string, info os.FileInfo, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if !info.Mode().IsRegular() {
			return nil
		}
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		files[filepath.ToSlash(relative)] = fmt.Sprintf("%x", sha256.Sum256([]byte(fold(string(data), oracleRoot, root))))
		return nil
	})
	if err != nil {
		return nil, err
	}
	return files, nil
}

// cliEnvironment isolates the process exactly like the recorded CLI fixture
// does: private home, private XDG root, UTC clock, C locale. PATH stays empty on
// purpose so no real scheduler binary can be reached.
func cliEnvironment(root string) []string {
	environment := []string{
		"HOME=" + filepath.Join(root, "home"),
		"XDG_CONFIG_HOME=" + filepath.Join(root, "xdg"),
		"TZ=UTC",
		"LC_ALL=C",
		"PATH=",
		// A real shell exports PWD and Go's `os.Getwd` prefers it over the
		// kernel's symlink-resolved view, so the generated wrappers carry this
		// literal path. Setting it keeps the capture deterministic.
		"PWD=" + root,
	}
	if runtime.GOOS == "windows" {
		environment = append(environment, "USERPROFILE="+filepath.Join(root, "home"))
	}
	return environment
}

func fail(format string, arguments ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", arguments...)
	os.Exit(1)
}
