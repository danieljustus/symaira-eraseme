// Command config is the committed Go configuration provenance oracle.
//
// The parent process creates isolated HOME/XDG/CWD/TEMP inputs under the
// platform's native temporary root and invokes the existing internal/config
// package in a child process. This keeps process-global state out of the Rust
// test while exercising the Go contract.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/config"
)

const (
	inputFile         = "inputs.json"
	sourceRevision    = "119ee9f84fe7c9e1485d25ab10aac8582e98395c"
	sourceSHA256      = "d197afc83776a85880428994e32b0c1585c245ed52d86f9a31fe51889cbce32c"
	oracleSchema      = "symaira-eraseme.config-parity.v1"
	maxChildOutput    = int64(4 * 1024 * 1024)
	childCommandLimit = 10 * time.Second
)

type provenance struct {
	SourceRevision string `json:"source_revision"`
	SourcePath     string `json:"source_path"`
	SourceSHA256   string `json:"source_sha256"`
	Schema         string `json:"schema"`
}

type fixtureEnvironment struct {
	// Values contains only non-reserved fixture overrides. The XDG sandbox is
	// represented separately so arbitrary absolute roots cannot enter a child.
	Values         map[string]string `json:"values,omitempty"`
	SandboxXDGPath string            `json:"sandbox_xdg_path,omitempty"`
}

type runInput struct {
	Files       map[string]string  `json:"files"`
	Environment fixtureEnvironment `json:"environment"`
}

type caseInput struct {
	Runs []runInput `json:"runs"`
}

type inputDocument struct {
	Provenance provenance           `json:"provenance"`
	Cases      map[string]caseInput `json:"cases"`
}

type childResult struct {
	Config     *config.Config  `json:"config,omitempty"`
	Storage    *config.Storage `json:"storage,omitempty"`
	ErrorField string          `json:"error_field,omitempty"`
}

type oracleOutput struct {
	Provenance provenance                 `json:"provenance"`
	Cases      map[string]json.RawMessage `json:"cases"`
}

type commandOutput struct {
	Stdout []byte
	Stderr []byte
}

func main() {
	if len(os.Args) == 3 && os.Args[1] == "--child" {
		runChild(os.Args[2])
	}

	source := oracleSourcePath()
	configSource := filepath.Clean(filepath.Join(filepath.Dir(source), "../../../../internal/config/config.go"))
	if err := verifySourceHash(configSource, sourceSHA256); err != nil {
		fatal("oracle source provenance verification failed")
	}
	inputs := readInputs()
	if inputs.Provenance.SourceRevision != sourceRevision ||
		inputs.Provenance.SourcePath != "internal/config/config.go" ||
		inputs.Provenance.SourceSHA256 != sourceSHA256 ||
		inputs.Provenance.Schema != oracleSchema {
		fatal("oracle input provenance does not match the committed contract")
	}

	cases := make(map[string]json.RawMessage, len(inputs.Cases))
	caseNames := make([]string, 0, len(inputs.Cases))
	for name := range inputs.Cases {
		caseNames = append(caseNames, name)
	}
	sort.Strings(caseNames)
	for _, name := range caseNames {
		result := runCase(name, inputs.Cases[name])
		encoded, err := json.Marshal(result)
		if err != nil {
			fatal("encode oracle result")
		}
		cases[name] = encoded
	}

	output := oracleOutput{Provenance: inputs.Provenance, Cases: cases}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fatal("write oracle result")
	}
}

func readInputs() inputDocument {
	contents, err := os.ReadFile(filepath.Join(filepath.Dir(oracleSourcePath()), inputFile))
	if err != nil {
		fatal("oracle inputs unavailable")
	}
	var inputs inputDocument
	if err := json.Unmarshal(contents, &inputs); err != nil {
		fatal("oracle inputs are invalid")
	}
	return inputs
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

func runCase(name string, input caseInput) map[string]any {
	root, err := os.MkdirTemp("", "symeraseme-config-oracle-")
	if err != nil {
		fatal("create oracle temporary root")
	}
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		fatal("resolve oracle temporary root")
	}
	defer os.RemoveAll(root)

	results := make([]childResult, 0, len(input.Runs))
	for index, run := range input.Runs {
		if err := resetRunTree(root); err != nil {
			fatal(fmt.Sprintf("reset oracle tree for %s/%d", name, index))
		}
		if err := writeRunFiles(root, run.Files); err != nil {
			fatal(fmt.Sprintf("write oracle files for %s/%d", name, index))
		}
		results = append(results, runChildProcess(root, run.Environment))
	}

	if name == "CFG-005" {
		errors := make([]string, 0, len(results))
		for _, result := range results {
			if result.ErrorField == "" {
				fatal("expected CFG-005 to contain only classified errors")
			}
			errors = append(errors, result.ErrorField)
		}
		return map[string]any{"errors": errors}
	}
	if len(results) != 1 || results[0].Config == nil || results[0].Storage == nil {
		fatal(fmt.Sprintf("case %s did not produce one successful result", name))
	}
	return map[string]any{
		"config":  results[0].Config,
		"storage": normalizePaths(root, results[0].Storage),
	}
}

func runChildProcess(root string, environment fixtureEnvironment) childResult {
	inputPath := filepath.Join(root, "child-input.json")
	encoded, err := json.Marshal(runInput{Environment: environment})
	if err != nil {
		fatal("encode child input")
	}
	if err := os.WriteFile(inputPath, encoded, 0o600); err != nil {
		fatal("write child input")
	}

	env, err := isolatedEnvironment(root, environment)
	if err != nil {
		fatal("reject unsafe child environment")
	}
	output, runErr := runBoundedCommand(
		context.Background(),
		os.Args[0],
		[]string{"--child", inputPath},
		filepath.Join(root, "project"),
		env,
		filepath.Join(root, "child.stdout"),
		filepath.Join(root, "child.stderr"),
		childCommandLimit,
	)
	if runErr != nil {
		var result childResult
		if json.Unmarshal(output.Stdout, &result) == nil && result.ErrorField != "" {
			return result
		}
		fatal("Go configuration child failed")
	}
	var result childResult
	if err := json.Unmarshal(output.Stdout, &result); err != nil {
		fatal("decode Go configuration child")
	}
	return result
}

func runBoundedCommand(
	parent context.Context,
	executable string,
	args []string,
	directory string,
	environment []string,
	stdoutPath string,
	stderrPath string,
	limit time.Duration,
) (commandOutput, error) {
	ctx, cancel := context.WithTimeout(parent, limit)
	defer cancel()

	stdout, err := os.OpenFile(stdoutPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return commandOutput{}, err
	}
	defer stdout.Close()
	stderr, err := os.OpenFile(stderrPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return commandOutput{}, err
	}
	defer stderr.Close()

	command := exec.CommandContext(ctx, executable, args...)
	command.Dir = directory
	command.Env = environment
	command.Stdout = stdout
	command.Stderr = stderr
	if err := configureProcessGroup(command); err != nil {
		return commandOutput{}, err
	}
	// CommandContext kills the direct child when the context expires. Replacing
	// that cancellation hook makes timeout cleanup cover the whole process tree.
	command.Cancel = func() error { return killProcessTree(command) }

	if err := command.Start(); err != nil {
		return commandOutput{}, err
	}
	done := make(chan error, 1)
	go func() { done <- command.Wait() }()
	var runErr error
	for {
		if commandOutputExceeded(stdoutPath, stderrPath, maxChildOutput) {
			if err := cleanupCommandBounded(command, done, killProcessTree, 2*time.Second); err != nil {
				return commandOutput{}, err
			}
			return commandOutput{}, fmt.Errorf("oracle child output exceeded the bounded capture limit")
		}
		select {
		case runErr = <-done:
			goto finished
		case <-ctx.Done():
			if err := cleanupCommandBounded(command, done, killProcessTree, 2*time.Second); err != nil {
				return commandOutput{}, err
			}
			return commandOutput{}, ctx.Err()
		default:
			time.Sleep(10 * time.Millisecond)
		}
	}

finished:
	captured, captureErr := readCommandOutput(stdoutPath, stderrPath, maxChildOutput)
	if captureErr != nil {
		return commandOutput{}, captureErr
	}
	if ctx.Err() != nil {
		return captured, ctx.Err()
	}
	if runErr != nil {
		return captured, runErr
	}
	return captured, nil
}

func cleanupCommandBounded(
	command *exec.Cmd,
	done <-chan error,
	treeKiller func(*exec.Cmd) error,
	waitLimit time.Duration,
) error {
	treeErr := treeKiller(command)
	var directErr error
	if command.Process != nil {
		directErr = command.Process.Kill()
	}
	select {
	case <-done:
	case <-time.After(waitLimit):
		return fmt.Errorf("oracle child cleanup exceeded the bounded wait")
	}
	if treeErr != nil {
		return fmt.Errorf("oracle process-tree cleanup failed: %w", treeErr)
	}
	if directErr != nil && !errors.Is(directErr, os.ErrProcessDone) {
		return fmt.Errorf("oracle direct-child cleanup failed: %w", directErr)
	}
	return nil
}

func commandOutputExceeded(stdoutPath, stderrPath string, limit int64) bool {
	for _, path := range []string{stdoutPath, stderrPath} {
		info, err := os.Stat(path)
		if err == nil && info.Size() > limit {
			return true
		}
	}
	return false
}

func readCommandOutput(stdoutPath, stderrPath string, limit int64) (commandOutput, error) {
	stdout, err := readCappedFile(stdoutPath, limit)
	if err != nil {
		return commandOutput{}, err
	}
	stderr, err := readCappedFile(stderrPath, limit)
	if err != nil {
		return commandOutput{}, err
	}
	return commandOutput{Stdout: stdout, Stderr: stderr}, nil
}

func readCappedFile(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	contents, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(contents)) > limit {
		return nil, fmt.Errorf("oracle child output exceeded the bounded capture limit")
	}
	return contents, nil
}

func runChild(path string) {
	// The input file is deliberately read to prove that the child received an
	// explicit run descriptor; configuration itself reads only the isolated env
	// and current directory, matching the production Go loader contract.
	if _, err := os.ReadFile(path); err != nil {
		fatal("read child input")
	}
	loader := config.Load()
	cfg, err := loader.Load()
	if err != nil {
		writeChildResult(childResult{ErrorField: classifyField(err)})
	}
	storage, err := config.ResolveStorage()
	if err != nil {
		writeChildResult(childResult{ErrorField: classifyField(err)})
	}
	writeChildResult(childResult{Config: cfg, Storage: &storage})
}

func writeChildResult(result childResult) {
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fatal("write child result")
	}
	os.Exit(0)
}

func classifyField(err error) string {
	message := err.Error()
	for _, field := range []string{"data_dir", "db_dir", "encrypt_db", "port", "allow_remote"} {
		if strings.Contains(message, field) {
			return field
		}
	}
	for name, field := range map[string]string{
		"SYMERASEME_DATA_DIR":     "data_dir",
		"SYMERASEME_DB_DIR":       "db_dir",
		"SYMERASEME_ENCRYPT_DB":   "encrypt_db",
		"SYMERASEME_PORT":         "port",
		"SYMERASEME_ALLOW_REMOTE": "allow_remote",
	} {
		if strings.Contains(message, name) {
			return field
		}
	}
	return "unknown"
}

func isolatedEnvironment(root string, fixture fixtureEnvironment) ([]string, error) {
	values := map[string]string{
		"HOME":        filepath.Join(root, "home"),
		"USERPROFILE": filepath.Join(root, "home"),
		"TMPDIR":      filepath.Join(root, "tmp"),
		"TEMP":        filepath.Join(root, "tmp"),
		"TMP":         filepath.Join(root, "tmp"),
	}
	if fixture.SandboxXDGPath != "" {
		if fixture.SandboxXDGPath != "$ROOT/xdg" {
			return nil, fmt.Errorf("sandbox XDG path is not the generated sandbox")
		}
		xdg := filepath.Join(root, "xdg")
		values["XDG_CONFIG_HOME"] = xdg
		values["XDG_DATA_HOME"] = xdg
		values["XDG_CACHE_HOME"] = xdg
	}
	for key, value := range fixture.Values {
		switch key {
		case "XDG_CONFIG_HOME":
			// A relative value exercises the production fallback. Absolute XDG
			// roots must use SandboxXDGPath instead.
			if fixture.SandboxXDGPath != "" {
				return nil, fmt.Errorf("fixture cannot override reserved environment")
			}
			expanded := strings.ReplaceAll(value, "$ROOT", root)
			if filepath.IsAbs(expanded) || isWindowsAbsolute(expanded) {
				return nil, fmt.Errorf("fixture cannot override reserved environment")
			}
			values[key] = expanded
		case "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_ENCRYPT_DB",
			"SYMERASEME_PORT", "SYMERASEME_ALLOW_REMOTE":
			values[key] = strings.ReplaceAll(value, "$ROOT", root)
		default:
			return nil, fmt.Errorf("fixture environment key is not allowed")
		}
	}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	env := make([]string, 0, len(keys))
	for _, key := range keys {
		env = append(env, key+"="+values[key])
	}
	return env, nil
}

func isWindowsAbsolute(path string) bool {
	return (len(path) >= 2 && path[1] == ':' &&
		((path[0] >= 'a' && path[0] <= 'z') || (path[0] >= 'A' && path[0] <= 'Z'))) ||
		strings.HasPrefix(path, `\\`)
}

func resetRunTree(root string) error {
	for _, name := range []string{"home", "project", "xdg", "tmp"} {
		if err := os.RemoveAll(filepath.Join(root, name)); err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Join(root, name), 0o700); err != nil {
			return err
		}
	}
	return nil
}

func writeRunFiles(root string, files map[string]string) error {
	for relative, contents := range files {
		if unsafeFixturePath(relative) {
			return fmt.Errorf("unsafe oracle input path")
		}
		path := filepath.Join(root, filepath.FromSlash(strings.ReplaceAll(relative, `\`, "/")))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
			return err
		}
	}
	return nil
}

func unsafeFixturePath(path string) bool {
	if path == "" || filepath.IsAbs(path) || isWindowsAbsolute(path) {
		return true
	}
	for _, component := range strings.Split(strings.ReplaceAll(path, `\`, "/"), "/") {
		if component == "" || component == "." || component == ".." {
			return true
		}
	}
	return false
}

func normalizePaths(root string, storage *config.Storage) *config.Storage {
	copy := *storage
	cacheRoot := nativeCacheRoot(copy.TempDir)
	copy.DataDir = normalizePath(root, "$ROOT", copy.DataDir)
	copy.DBDir = normalizePath(root, "$ROOT", copy.DBDir)
	copy.DBPath = normalizePath(root, "$ROOT", copy.DBPath)
	copy.TempDir = normalizePath(cacheRoot, "$CACHE", copy.TempDir)
	return &copy
}

func nativeCacheRoot(tempDir string) string {
	if filepath.Base(tempDir) != "database" {
		return ""
	}
	toolDir := filepath.Dir(tempDir)
	if filepath.Base(toolDir) != "symeraseme" {
		return ""
	}
	return filepath.Dir(toolDir)
}

func normalizePath(root, placeholder, value string) string {
	value = filepath.ToSlash(value)
	root = strings.TrimRight(filepath.ToSlash(root), "/")
	if root == "" {
		return value
	}
	if value == root {
		return placeholder
	}
	if suffix, ok := strings.CutPrefix(value, root); ok && strings.HasPrefix(suffix, "/") {
		return placeholder + suffix
	}
	return value
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
